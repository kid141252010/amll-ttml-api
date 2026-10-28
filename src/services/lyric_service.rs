//! 歌词业务层，负责六个数据端点背后的业务操作
//!
//! - `search_lyric` —> `/lyrics/search`：元数据模糊匹配与歌词正文 FTS 检索，两路命中合并排序后分页
//! - `list_lyrics` —> `/lyrics/list`：不带检索条件的列表，支持结构化过滤，并按创建时间、ID、曲名、
//!   艺术家名或专辑名排序后分页
//! - `get_lyric` —> `/lyrics/get`：按 53 位 ID、文件名或平台 ID 定位歌曲，返回条目与歌词原文
//! - `lrclib_search` —> `/lrclib/search`：LRCLIB 兼容搜索，逐条附带解析后的歌词
//! - `lrclib_get_by_fields` / `lrclib_get_by_id` -> `/lrclib/get`：按字段组合或 ID
//!   取单条并附带解析后的歌词
//!
//! 本层只产出领域数据（歌曲条目与歌词内容），分页也在此完成；
//! JSON DTO 的映射与响应包装由 api 层的 handler 负责

use compact_str::CompactString;
use futures::StreamExt;

use crate::{
    core::{
        LyricId,
        error::AppError,
        list_query::{
            Cursor,
            ListOrder,
            ListQuery,
            ListSort,
        },
        matcher::MatchType,
        models::{
            IdQuery,
            LyricHit,
            LyricSearchResult,
            SearchQuery,
            SongEntry,
        },
        pagination::{
            Paginated,
            Pagination,
            PaginationInfo,
            paginate,
        },
        repository::MetadataHit,
    },
    services::{
        LyricStore,
        ranking::merge_and_sort_hits,
    },
    utils::ttml::TTMLFormatResult,
};

/// FTS 候选窗口相对页大小的冗余倍数
///
/// FTS 命中需要和元数据命中合并并去重，窗口里相当一部分条目会跟元数据重复，
/// 保留冗余才能保证合并后仍有足够候选供翻页
const FTS_WINDOW_FACTOR: u64 = 3;
/// FTS 候选窗口上限，防止深翻页把 SQL LIMIT 撑到不可控
const FTS_WINDOW_CAP: u64 = 500;
/// 元数据粗筛命中数量低于此阈值时，认为元数据命中所选偏弱，触发 FTS5 正文检索补全
const FTS_TRIGGER_MIN_HITS: usize = 10;
/// lrclib 搜索逐条拉取解析歌词的并发上限
const CONCURRENT_FETCH_LIMIT: usize = 10;

/// 按翻页深度动态放大 FTS 候选窗口，并限制到 `FTS_WINDOW_CAP`
fn fts_window(pagination: Pagination) -> u64 {
    pagination
        .page
        .saturating_mul(pagination.page_size)
        .saturating_mul(FTS_WINDOW_FACTOR)
        .min(FTS_WINDOW_CAP)
}

pub async fn search_lyric<R: LyricStore>(
    store: &R,
    query: &SearchQuery,
    pagination: Pagination,
) -> Paginated<LyricSearchResult> {
    let db = store.load_index().await;
    let metadata_hits = db.search_by_fields(query);

    let lyric_hits =
        fetch_lyric_hits_if_needed(store, query, &metadata_hits, fts_window(pagination)).await;

    let sorted_hits =
        merge_and_sort_hits(&db, metadata_hits, lyric_hits, query.lyric_text.is_some());

    paginate(sorted_hits, pagination, |hit| LyricSearchResult {
        entry: hit.entry.clone(),
        lyric_hit: hit.lyric_hit,
    })
}

/// 歌词列表，按请求指定的维度和方向排序后分页，可带结构化过滤
///
/// 过滤条件（作者、`hasId` / `missingId`）由 `LyricIndexDB::list_candidates` 判定，
/// 分页的 `total` 因此是过滤后的数量；条件全空时退化为全量列表
///
/// 排序键是 `SongEntry::timestamp`，即上游 CI 处理投稿时打的戳，不是合并进词库的时刻——
/// 上游 PR 可能拖数月才合并，因此本列表不等于「最近新增到词库」的顺序
///
/// 时间戳同值时用 `id` 兜底全序。timestamp 派生自上游文件名的毫秒前缀，同一批投稿
/// 完全可能同值，而 `sort_unstable_by` 不保证同键条目的相对顺序；少了兜底键，
/// 索引重建后相邻页之间会重复或漏掉条目
pub async fn list_lyrics<R: LyricStore>(store: &R, query: ListQuery) -> Paginated<SongEntry> {
    let db = store.load_index().await;

    let mut ordered: Vec<&SongEntry> = db
        .list_candidates(&query.filter)
        .into_iter()
        .map(|idx| &db.entries[idx])
        .collect();
    ordered.sort_unstable_by(|a, b| {
        let primary = match query.sort {
            ListSort::CreatedAt => a.timestamp.cmp(&b.timestamp),
            ListSort::Id => a.id.cmp(&b.id),
            ListSort::TrackName => first_name(&a.track_names).cmp(first_name(&b.track_names)),
            ListSort::ArtistName => first_name(&a.artist_names).cmp(first_name(&b.artist_names)),
            ListSort::AlbumName => first_name(&a.album_names).cmp(first_name(&b.album_names)),
        };
        let primary = match query.order {
            ListOrder::Desc => primary.reverse(),
            ListOrder::Asc => primary,
        };
        // 私库优先。游标分页要求显式 source（解析层已保证），此时恒为 tie，
        // 不影响游标的 (timestamp, id) 二分
        a.source
            .rank()
            .cmp(&b.source.rank())
            .then_with(|| primary)
            .then_with(|| a.id.cmp(&b.id))
    });

    let total = u64::try_from(ordered.len()).unwrap_or(u64::MAX);

    if let Some(cursor) = query.cursor {
        let start_idx = match query.order {
            ListOrder::Desc => ordered.partition_point(|entry| {
                entry.timestamp > cursor.timestamp
                    || (entry.timestamp == cursor.timestamp && entry.id.get() <= cursor.id.get())
            }),
            ListOrder::Asc => ordered.partition_point(|entry| {
                entry.timestamp < cursor.timestamp
                    || (entry.timestamp == cursor.timestamp && entry.id.get() <= cursor.id.get())
            }),
        };

        let page_size = usize::try_from(query.pagination.page_size).unwrap_or(usize::MAX);
        let end_idx = start_idx.saturating_add(page_size).min(ordered.len());
        let paged_items: Vec<SongEntry> = ordered[start_idx..end_idx]
            .iter()
            .map(|&entry| entry.clone())
            .collect();

        let has_more = end_idx < ordered.len();
        let next_cursor = if has_more {
            paged_items
                .last()
                .map(|last| Cursor::from_entry(last).to_compact_string())
        } else {
            None
        };

        Paginated {
            items: paged_items,
            pagination: PaginationInfo {
                page: None,
                page_size: query.pagination.page_size,
                total,
                total_pages: None,
                has_more,
                next_cursor,
            },
        }
    } else {
        let mut paginated = paginate(ordered, query.pagination, Clone::clone);
        if paginated.pagination.has_more {
            paginated.pagination.next_cursor = paginated
                .items
                .last()
                .map(|last| Cursor::from_entry(last).to_compact_string());
        }
        paginated
    }
}

fn first_name(names: &[CompactString]) -> &str {
    names.first().map_or("", |name| name.as_str())
}

async fn fetch_lyric_hits_if_needed<R: LyricStore>(
    store: &R,
    query: &SearchQuery,
    metadata_hits: &[MetadataHit<'_>],
    fts_window: u64,
) -> Vec<LyricHit> {
    // 计算仅凭元数据搜索的结果是否较弱，如果较低，或者结果较少，我们需要继续匹配歌词正文
    // 如果元数据已经足够精确了，并且用户没有明确要求去查歌词正文，那我们就不用继续匹配正文了
    let is_weak = metadata_hits.is_empty()
        || metadata_hits
            .first()
            .is_none_or(|h| h.score < MatchType::Medium)
        || metadata_hits.len() < FTS_TRIGGER_MIN_HITS;

    let fts_keyword = match (&query.lyric_text, &query.global_keyword) {
        (Some(explicit), _) => Some(explicit.clone()),
        (None, Some(global_q)) if is_weak => Some(global_q.clone()),
        _ => None,
    };

    if let Some(ref kw) = fts_keyword {
        match store.search_lyrics_fts(kw, fts_window).await {
            Ok(hits) => hits,
            Err(e) => {
                tracing::error!("SQLite FTS5 lyric search failed for keyword '{kw}': {e:?}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    }
}

pub async fn get_lyric<R: LyricStore>(
    store: &R,
    query: IdQuery,
) -> Result<(SongEntry, String), AppError> {
    let db = store.load_index().await;
    let matched_indices = db.find_by_ids(&query);

    if matched_indices.is_empty() {
        return Err(AppError::LyricNotFound);
    }

    let mut candidates: Vec<_> = matched_indices
        .into_iter()
        .map(|idx| &db.entries[idx])
        .collect();
    // 私库优先：同 source 内再取时间戳最新的
    candidates.sort_by(|a, b| {
        a.source
            .rank()
            .cmp(&b.source.rank())
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    let latest_song_cloned = candidates[0].clone();
    drop(db);

    let ttml_text = store
        .fetch_lyric_ttml(latest_song_cloned.filename.as_str())
        .await?;

    Ok((latest_song_cloned, ttml_text))
}

/// LRCLIB 兼容搜索，逐条附带解析后的歌词
///
/// 对外响应是裸数组，分页元数据只供请求统计取命中总数
pub async fn lrclib_search<R: LyricStore>(
    store: &R,
    query: &SearchQuery,
    pagination: Pagination,
) -> Paginated<(SongEntry, Option<TTMLFormatResult>)> {
    let db = store.load_index().await;

    let matched_hits = db.search_by_fields(query);
    let paginated = paginate(matched_hits, pagination, |hit| hit.entry.clone());

    drop(db);

    let items = futures::stream::iter(paginated.items)
        .map(|entry| async move {
            let formatted = match store.fetch_parsed_lyric(entry.filename.as_str()).await {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch parsed lyric for file '{}' in LRCLIB search: {e:?}",
                        entry.filename
                    );
                    None
                }
            };
            (entry, formatted)
        })
        .buffered(CONCURRENT_FETCH_LIMIT)
        .collect()
        .await;

    Paginated {
        items,
        pagination: paginated.pagination,
    }
}

pub async fn lrclib_get_by_fields<R: LyricStore>(
    store: &R,
    query: &SearchQuery,
) -> Result<(SongEntry, TTMLFormatResult), AppError> {
    let db = store.load_index().await;
    let matched_hits = db.search_by_fields(query);

    if matched_hits.is_empty() {
        return Err(AppError::LyricNotFound);
    }

    let best_hit = &matched_hits[0];

    if best_hit.score < MatchType::Medium {
        return Err(AppError::LyricNotFound);
    }

    let latest_song_cloned = best_hit.entry.clone();
    drop(db);

    let formatted = store
        .fetch_parsed_lyric(latest_song_cloned.filename.as_str())
        .await?;
    Ok((latest_song_cloned, formatted))
}

pub async fn lrclib_get_by_id<R: LyricStore>(
    store: &R,
    id: LyricId,
) -> Result<(SongEntry, TTMLFormatResult), AppError> {
    let db = store.load_index().await;

    let idx = db.id_idx.get(&id).copied().ok_or(AppError::LyricNotFound)?;
    let song_cloned = db.entries[idx].clone();
    drop(db);

    let formatted = store
        .fetch_parsed_lyric(song_cloned.filename.as_str())
        .await?;
    Ok((song_cloned, formatted))
}

#[cfg(test)]
#[path = "lyric_service_tests.rs"]
mod tests;
