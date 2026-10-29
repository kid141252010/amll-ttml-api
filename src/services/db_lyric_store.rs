//! 生产环境的歌词存储库，负责获取和更新歌词数据

use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};

use arc_swap::ArcSwap;
use moka::future::Cache;
use reqwest::Client;
use sea_orm::DatabaseConnection;

use crate::{
    core::{
        LyricId,
        db::queries::{
            fetch_fts_snippets,
            fetch_song_entries,
            find_raw_ttml,
            search_fts,
        },
        error::AppError,
        models::{
            FtsFilter,
            LyricHit,
            LyricIndexDB,
        },
    },
    services::LyricStore,
    utils::ttml::{
        TTMLFormatResult,
        parse_and_format_ttml,
    },
};

#[derive(Clone)]
pub struct DbLyricStore {
    pub index: Arc<ArcSwap<LyricIndexDB>>,
    pub conn: DatabaseConnection,
    pub client: Client,
    pub ttml_cache: Cache<String, String>,
    pub formatted_lyric_cache: Cache<String, TTMLFormatResult>,
}

impl DbLyricStore {
    /// 构造一个从空索引启动的存储
    #[must_use]
    pub fn new(conn: DatabaseConnection) -> Self {
        let client = Client::builder()
            .user_agent("amll-ttml-api/0.1")
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        let ttml_cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_hours(14 * 24))
            .build();

        let formatted_lyric_cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_hours(14 * 24))
            .build();

        Self {
            index: Arc::new(ArcSwap::from_pointee(LyricIndexDB::default())),
            conn,
            client,
            ttml_cache,
            formatted_lyric_cache,
        }
    }

    /// 当前索引条目数
    #[must_use]
    pub fn lyric_count(&self) -> usize {
        self.index.load().entries.len()
    }

    /// 原子写入新索引
    pub fn swap_index(&self, new_index: LyricIndexDB) {
        self.index.store(Arc::new(new_index));
    }

    /// 使所有的缓存失效
    pub fn invalidate_caches(&self) {
        self.ttml_cache.invalidate_all();
        self.formatted_lyric_cache.invalidate_all();
    }

    /// 从 SQLite 重建内存索引并原子换入，索引是 SQLite 的派生缓存视图
    ///
    /// # Errors
    ///
    /// 查询或组装失败时返回错误，旧索引保持不变
    pub async fn rebuild_index(&self) -> Result<(), AppError> {
        let entries = fetch_song_entries(&self.conn).await?;
        let index = tokio::task::spawn_blocking(move || LyricIndexDB::from_entries(entries))
            .await
            .map_err(|e| AppError::InternalServerError(format!("Index assembly failed: {e}")))?;
        self.swap_index(index);
        Ok(())
    }

    /// 根据文件名获取 TTML 文件内容
    ///
    /// 获取顺序是 缓存 -> 本地数据库 -> GitHub 获取兜底
    pub async fn fetch_lyric_ttml(&self, filename: &str) -> Result<String, AppError> {
        if let Some(cached) = self.ttml_cache.get(filename).await {
            return Ok(cached);
        }

        if let Some(ttml) = find_raw_ttml(&self.conn, filename).await {
            self.ttml_cache
                .insert(filename.to_string(), ttml.clone())
                .await;
            return Ok(ttml);
        }

        let text = fetch_raw_lyric(&self.client, filename).await?;
        self.ttml_cache
            .insert(filename.to_string(), text.clone())
            .await;
        Ok(text)
    }

    /// 根据文件名获取解析后的 [`TTMLFormatResult`]，包含纯文本歌词、LRC 歌词和时长
    pub async fn fetch_parsed_lyric(&self, filename: &str) -> Result<TTMLFormatResult, AppError> {
        if let Some(cached) = self.formatted_lyric_cache.get(filename).await {
            return Ok(cached);
        }

        let ttml = self.fetch_lyric_ttml(filename).await?;
        let formatted = parse_and_format_ttml(&ttml);

        self.formatted_lyric_cache
            .insert(filename.to_string(), formatted.clone())
            .await;

        Ok(formatted)
    }

    /// FTS 全文检索
    pub async fn search_lyrics_fts(
        &self,
        keyword: &str,
        filter: &FtsFilter,
    ) -> Result<Vec<LyricHit>, AppError> {
        search_fts(&self.conn, keyword, filter).await
    }

    /// 为一页 FTS 命中补取 snippet
    pub async fn fetch_lyric_snippets(
        &self,
        keyword: &str,
        ids: &[LyricId],
    ) -> Result<HashMap<LyricId, String>, AppError> {
        fetch_fts_snippets(&self.conn, keyword, ids).await
    }
}

/// 从 GitHub 根据指定文件名下载歌词内容
///
/// 只有在缓存和本地数据库都不存在时才应使用
async fn fetch_raw_lyric(client: &Client, filename: &str) -> Result<String, AppError> {
    let ttml_url = format!(
        "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main/raw-lyrics/{filename}"
    );

    let res = client.get(&ttml_url).send().await?;
    if !res.status().is_success() {
        return Err(AppError::UpstreamError(
            "Failed to fetch lyric file from GitHub".into(),
        ));
    }

    let text = res.text().await?;
    Ok(text)
}

#[allow(clippy::unused_async_trait_impl)]
impl LyricStore for DbLyricStore {
    async fn fetch_lyric_ttml(&self, filename: &str) -> Result<String, AppError> {
        self.fetch_lyric_ttml(filename).await
    }

    async fn fetch_parsed_lyric(&self, filename: &str) -> Result<TTMLFormatResult, AppError> {
        self.fetch_parsed_lyric(filename).await
    }

    async fn search_lyrics_fts(
        &self,
        keyword: &str,
        filter: &FtsFilter,
    ) -> Result<Vec<LyricHit>, AppError> {
        self.search_lyrics_fts(keyword, filter).await
    }

    async fn fetch_lyric_snippets(
        &self,
        keyword: &str,
        ids: &[LyricId],
    ) -> Result<HashMap<LyricId, String>, AppError> {
        self.fetch_lyric_snippets(keyword, ids).await
    }

    async fn load_index(&self) -> Arc<LyricIndexDB> {
        self.index.load_full()
    }
}

#[cfg(test)]
#[path = "db_lyric_store_tests.rs"]
mod tests;
