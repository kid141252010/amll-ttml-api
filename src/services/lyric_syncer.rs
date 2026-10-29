//! 数据同步器，负责把 GitHub 词库的最新数据同步到本地的数据库
//!
//! 触发源有三个：
//! 1. 进程启动
//! 2. 每 24 小时的定时任务
//! 3. webhook/sync API 调用
//!
//! 并发触发时只允许一个同步在途，其余直接跳过
//!
//! 每次同步依次执行：
//! 1. 从远端下载并写入 SQLite（`SyncService` 负责）
//! 2. 从 SQLite 重建内存索引
//! 3. 清空歌词缓存
//!
//! 索引是 SQLite 的派生视图，重建失败时保留旧索引继续服务，与库的落差最迟在下一次同步收敛。

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
};

use tokio::sync::Mutex;
use tracing::{
    error,
    info,
    warn,
};

use crate::{
    core::error::AppError,
    services::{
        db_lyric_store::DbLyricStore,
        sync_service::{
            SyncResult,
            SyncService,
            SyncStatus,
        },
    },
};

type SyncFuture = Pin<Box<dyn Future<Output = anyhow::Result<SyncResult>> + Send>>;

/// 「从远端写入 SQLite」这一步，参数是 `force`
///
/// 生产实现是 [`SyncService::sync`]；抽成可注入的函数，测试可以换成不出网的假实现
type SyncRunner = Arc<dyn Fn(bool) -> SyncFuture + Send + Sync>;

#[derive(Clone)]
pub struct LyricSyncer {
    store: DbLyricStore,
    lock: Arc<Mutex<()>>,
    runner: SyncRunner,
}

impl LyricSyncer {
    #[must_use]
    pub fn new(store: DbLyricStore) -> Self {
        let conn = store.conn.clone();
        let client = store.client.clone();
        Self::with_runner(store, move |force| {
            let syncer = SyncService::new(conn.clone(), client.clone());
            Box::pin(async move { syncer.sync(force).await })
        })
    }

    /// 用自定义的同步步骤构造，锁、索引重建与缓存失效的编排逻辑不变
    #[must_use]
    pub fn with_runner(
        store: DbLyricStore,
        runner: impl Fn(bool) -> SyncFuture + Send + Sync + 'static,
    ) -> Self {
        Self {
            store,
            lock: Arc::new(Mutex::new(())),
            runner: Arc::new(runner),
        }
    }

    /// 触发一次同步；已有同步在途时跳过（不排队）
    ///
    /// 如果 force 为 true，将会触发强制全量同步
    ///
    /// # Errors
    ///
    /// SQLite 同步失败时返回 [`AppError::UpstreamError`]；索引重建失败不报错，
    /// 保留旧索引继续服务
    pub async fn sync(&self, force: bool) -> Result<(), AppError> {
        let Ok(_guard) = self.lock.try_lock() else {
            info!("Database sync is already in progress, skipping duplicate request.");
            return Ok(());
        };

        info!(force, "Running database sync service...");
        match (self.runner)(force).await {
            Ok(res) => {
                info!("Sync completed with status: {:?}", res.status);
                if res.status == SyncStatus::Updated {
                    info!(
                        "Database was updated with new entries, rebuilding index and invalidating caches."
                    );
                    match self.store.rebuild_index().await {
                        Ok(()) => self.store.invalidate_caches(),
                        Err(e) => {
                            warn!("Index rebuild failed, keeping previous index: {e:?}");
                        }
                    }
                } else {
                    info!("Database was not changed, keeping existing in-memory index and caches.");
                }
                Ok(())
            }
            Err(e) => {
                error!("Sync failed: {e:?}");
                Err(AppError::UpstreamError(format!("{e:#}")))
            }
        }
    }
}

#[cfg(test)]
#[path = "lyric_syncer_tests.rs"]
mod tests;
