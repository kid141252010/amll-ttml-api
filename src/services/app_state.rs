//! 应用全局状态
//!
//! 进程内共享的一组句柄，在启动时构造，经 axum 的 `State` 提取器注入各 handler
//!
//! 持有六样东西：
//! 1. 歌词存储 [`DbLyricStore`]（取词、FTS 检索、内存索引）
//! 2. 数据同步器 [`LyricSyncer`]（从远端仓库刷新 SQLite 并重建索引）
//! 3. 启动时刻（status 端点报告正常运行时间用）
//! 4. webhook 鉴权密钥
//! 5. 私库上传接口的鉴权密钥
//! 6. 请求统计句柄 [`Analytics`]（未启用时为空，统计中间件随之不挂载）

use compact_str::CompactString;
use sea_orm::DatabaseConnection;

use crate::{
    analytics::Analytics,
    services::{
        db_lyric_store::DbLyricStore,
        lyric_syncer::LyricSyncer,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub store: DbLyricStore,
    pub syncer: LyricSyncer,
    pub start_time: std::time::Instant,
    pub sync_secret: Option<CompactString>,
    pub admin_secret: Option<CompactString>,
    pub analytics: Option<Analytics>,
}

impl AppState {
    /// 从环境变量 `SYNC_SECRET` / `ADMIN_SECRET` 读取密钥构造
    pub fn new(db_conn: DatabaseConnection) -> Self {
        let sync_secret = read_secret("SYNC_SECRET");
        let admin_secret = read_secret("ADMIN_SECRET");

        let store = DbLyricStore::new(db_conn);
        let syncer = LyricSyncer::new(store.clone());
        Self {
            store,
            syncer,
            start_time: std::time::Instant::now(),
            sync_secret: sync_secret.map(CompactString::from),
            admin_secret: admin_secret.map(CompactString::from),
            analytics: None,
        }
    }

    /// 显式指定同步密钥构造（测试注入用）
    pub fn new_with_secret(db_conn: DatabaseConnection, secret: Option<&str>) -> Self {
        let store = DbLyricStore::new(db_conn);
        let syncer = LyricSyncer::new(store.clone());
        Self {
            store,
            syncer,
            start_time: std::time::Instant::now(),
            sync_secret: secret.map(CompactString::new),
            admin_secret: None,
            analytics: None,
        }
    }
    /// 启用请求统计
    #[must_use]
    pub fn with_analytics(mut self, analytics: Analytics) -> Self {
        self.analytics = Some(analytics);
        self
    }

    /// 注入私库上传接口的鉴权密钥（测试用）
    #[must_use]
    pub fn with_admin_secret(mut self, secret: &str) -> Self {
        self.admin_secret = Some(CompactString::new(secret));
        self
    }
}

/// 读密钥类环境变量：去首尾空白，空值视为未配置
fn read_secret(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
