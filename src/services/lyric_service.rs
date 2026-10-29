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
            FtsFilter,
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

/// 元数据粗筛命中数量低于此阈值时，认为元数据命中所选偏弱，触发 FTS5 正文检索补全
const FTS_TRIGGER_MIN_HITS: usize = 10;
/// lrclib 搜索逐条拉取解析歌词的并发上限
const CONCURRENT_FETCH_LIMIT: usize = 10;

/// 歌词搜索：元数据命中与 FTS 正文命中合并、全序排序后分页
///
/// FTS 取的是满足来源 / 作者条件的**完整**命中集（条件在 SQL 里与 MATCH 同时生效），
/// 不按页截断窗口，所以下面的合并作用在完整集合上，`total` 是真实匹配数：
/// - 显式 `lyricText`：元数据命中 ∩ 正文命中（元数据条件全空时元数据命中即全库，
///   退化为全部正文命中）
/// - `q` 降级触发的正文补全：元数据命中 ∪ 正文命中，按 id 去重
///
/// 正文命中只带 id / rank / 命中列，snippet 在分页之后只为本页条目补取
pub async fn search_lyric<R: LyricStore>(
    store: &R,
    query: &SearchQuery,
    pagination: Pagination,
) -> Paginated<LyricSearchResult> {
    let db = store.load_index().await;
    let metadata_hits = db.search_by_fields(query);

    let fts_keyword = fts_keyword_if_needed(query, &metadata_hits);
    let lyric_hits = match fts_keyword {
        Some(kw) => fetch_lyric_hits(store, kw, &FtsFilter::from(query)).await,
        None => Vec::new(),
    };

    let sorted_hits =
        merge_and_sort_hits(&db, metadata_hits, lyric_hits, query.lyric_text.is_some());

    let mut paginated = paginate(sorted_hits, pagination, |hit| LyricSearchResult {
        entry: hit.entry.clone(),
        lyric_hit: hit.lyric_hit,
    });
    drop(db);

    if let Some(kw) = fts_keyword {
        attach_snippets(store, kw, &mut paginated.items).await;
    }

    paginated
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

/// 决定是否需要 FTS 正文检索，需要时返回所用关键词
fn fts_keyword_if_needed<'q>(
    query: &'q SearchQuery,
    metadata_hits: &[MetadataHit<'_>],
) -> Option<&'q str> {
    // 计算仅凭元数据搜索的结果是否较弱，如果较低，或者结果较少，我们需要继续匹配歌词正文
    // 如果元数据已经足够精确了，并且用户没有明确要求去查歌词正文，那我们就不用继续匹配正文了
    let is_weak = metadata_hits.is_empty()
        || metadata_hits
            .first()
            .is_none_or(|h| h.score < MatchType::Medium)
        || metadata_hits.len() < FTS_TRIGGER_MIN_HITS;

    match (&query.lyric_text, &query.global_keyword) {
        (Some(explicit), _) => Some(explicit.as_str()),
        (None, Some(global_q)) if is_weak => Some(global_q.as_str()),
        _ => None,
    }
}

async fn fetch_lyric_hits<R: LyricStore>(
    store: &R,
    keyword: &str,
    filter: &FtsFilter,
) -> Vec<LyricHit> {
    match store.search_lyrics_fts(keyword, filter).await {
        Ok(hits) => hits,
        Err(e) => {
            tracing::error!("SQLite FTS5 lyric search failed for keyword '{keyword}': {e:?}");
            Vec::new()
        }
    }
}

/// 为本页带正文命中的条目补上 snippet；补取失败时 snippet 留空，不影响命中本身
async fn attach_snippets<R: LyricStore>(store: &R, keyword: &str, items: &mut [LyricSearchResult]) {
    let ids: Vec<LyricId> = items
        .iter()
        .filter(|item| item.lyric_hit.is_some())
        .map(|item| item.entry.id)
        .collect();
    if ids.is_empty() {
        return;
    }

    let mut snippets = match store.fetch_lyric_snippets(keyword, &ids).await {
        Ok(snippets) => snippets,
        Err(e) => {
            tracing::error!("SQLite FTS5 snippet fetch failed for keyword '{keyword}': {e:?}");
            return;
        }
    };

    for item in items {
        if let Some(ref mut lyric_hit) = item.lyric_hit {
            lyric_hit.snippet = snippets.remove(&item.entry.id);
        }
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
