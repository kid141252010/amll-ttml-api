use std::{
    collections::HashMap,
    sync::Arc,
};

use crate::{
    core::{
        LyricId,
        error::AppError,
        models::{
            FtsFilter,
            LyricHit,
            LyricIndexDB,
        },
    },
    utils::ttml::TTMLFormatResult,
};

pub trait LyricStore: Send + Sync {
    async fn fetch_lyric_ttml(&self, filename: &str) -> Result<String, AppError>;
    async fn fetch_parsed_lyric(&self, filename: &str) -> Result<TTMLFormatResult, AppError>;
    /// 返回满足 `filter` 的全部正文命中（不截断），`snippet` 一律为 `None`
    async fn search_lyrics_fts(
        &self,
        keyword: &str,
        filter: &FtsFilter,
    ) -> Result<Vec<LyricHit>, AppError>;
    /// 为给定条目补取 snippet，抽不出上下文的条目不出现在结果里
    async fn fetch_lyric_snippets(
        &self,
        keyword: &str,
        ids: &[LyricId],
    ) -> Result<HashMap<LyricId, String>, AppError>;
    async fn load_index(&self) -> Arc<LyricIndexDB>;
}
