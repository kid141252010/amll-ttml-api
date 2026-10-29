//! SQLite 查询层
//!
//! 负责 FTS 全文检索、读取原始 TTML 文件、读取重建内存索引所需的元数据条目
//!
//! 查询与 `setup.rs` 中的 FTS DDL（列序、trigram tokenizer、同步触发器）必须保持同步，
//! 修改任何一边必须要同步另一边

use std::collections::HashMap;

use compact_str::CompactString;
use sea_orm::{
    ColumnTrait,
    ConnectionTrait,
    DatabaseBackend,
    DatabaseConnection,
    EntityTrait,
    FromQueryResult,
    QueryFilter,
    QuerySelect,
    Statement,
};
use serde_json::Value;
use tracing::warn;

use crate::{
    core::{
        LyricId,
        db::entity,
        error::AppError,
        matcher::convert_tw2s,
        models::{
            FtsFilter,
            LyricHit,
            LyricMatchField,
            LyricSource,
            SongEntry,
        },
    },
    utils::highlight::extract_lyric_context,
};

/// 按文件名读取数据库中的 TTML 文件，空内容视为缺失
///
/// 调用方需要从 GitHub 获取以兜底
pub async fn find_raw_ttml(conn: &DatabaseConnection, filename: &str) -> Option<String> {
    let row = entity::Entity::find()
        .filter(entity::Column::Filename.eq(filename))
        .one(conn)
        .await
        .ok()??;

    let ttml = row.raw_ttml;
    (!ttml.is_empty()).then_some(ttml)
}

/// 读取全部条目的元数据，用于重建内存索引
///
/// 只投影元数据列，`raw_ttml`、`lyric_text`、`bg_vocal_text` 不进行重建，避免短时大量内存开销
pub async fn fetch_song_entries(conn: &DatabaseConnection) -> Result<Vec<SongEntry>, AppError> {
    let rows = entity::Entity::find()
        .select_only()
        .columns([
            entity::Column::Id,
            entity::Column::Filename,
            entity::Column::Timestamp,
            entity::Column::TrackNames,
            entity::Column::ArtistNames,
            entity::Column::AlbumNames,
            entity::Column::NormalizedTrackNames,
            entity::Column::NormalizedArtistNames,
            entity::Column::NormalizedAlbumNames,
            entity::Column::NcmMusicIds,
            entity::Column::QqMusicIds,
            entity::Column::AppleMusicIds,
            entity::Column::SpotifyIds,
            entity::Column::Isrcs,
            entity::Column::AuthorIds,
            entity::Column::AuthorUsernames,
            entity::Column::Source,
        ])
        .into_model::<MetadataRow>()
        .all(conn)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to load entries: {e}")))?;

    Ok(rows.into_iter().map(SongEntry::from).collect())
}

/// [`fetch_song_entries`] 的投影行形状，字段名与列名一致
#[derive(FromQueryResult)]
struct MetadataRow {
    id: i64,
    filename: String,
    timestamp: i64,
    track_names: Value,
    artist_names: Value,
    album_names: Value,
    normalized_track_names: Value,
    normalized_artist_names: Value,
    normalized_album_names: Value,
    ncm_music_ids: Value,
    qq_music_ids: Value,
    apple_music_ids: Value,
    spotify_ids: Value,
    isrcs: Value,
    author_ids: Value,
    author_usernames: Value,
    source: String,
}

impl From<MetadataRow> for SongEntry {
    fn from(row: MetadataRow) -> Self {
        Self {
            id: LyricId::from_u64_masked(row.id.cast_unsigned()),
            filename: CompactString::new(row.filename),
            timestamp: row.timestamp.cast_unsigned(),
            track_names: json_to_compact_box(row.track_names),
            artist_names: json_to_compact_box(row.artist_names),
            album_names: json_to_compact_box(row.album_names),
            normalized_track_names: json_to_compact_box(row.normalized_track_names),
            normalized_artist_names: json_to_compact_box(row.normalized_artist_names),
            normalized_album_names: json_to_compact_box(row.normalized_album_names),
            ncm_music_ids: json_to_compact_box(row.ncm_music_ids),
            qq_music_ids: json_to_compact_box(row.qq_music_ids),
            apple_music_ids: json_to_compact_box(row.apple_music_ids),
            spotify_ids: json_to_compact_box(row.spotify_ids),
            isrcs: json_to_compact_box(row.isrcs),
            author_ids: json_to_compact_box(row.author_ids),
            author_usernames: json_to_compact_box(row.author_usernames),
            source: LyricSource::from(row.source.as_str()),
        }
    }
}

fn json_to_compact_box(value: Value) -> Box<[CompactString]> {
    serde_json::from_value::<Vec<String>>(value)
        .unwrap_or_default()
        .into_iter()
        .map(CompactString::from)
        .collect()
}

/// 关键词归一化：繁简转换与小写化，与入库侧口径一致；空白关键词返回 `None`
fn normalize_fts_keyword(keyword: &str) -> Option<String> {
    if keyword.trim().is_empty() {
        return None;
    }
    Some(convert_tw2s(keyword).to_lowercase())
}

/// FTS5 全文检索歌词正文，返回满足 `filter` 的**全部**命中，按 bm25 相关度排序
///
/// 关键词先做繁简转换与小写化，双引号转义后以短语形式传给 MATCH
///
/// 不设 LIMIT：调用方要拿完整命中集与元数据命中做交集 / 并集，再按自己的全序分页。
/// 截断窗口会让过滤和 `total` 只作用在窗口内——窗口外的私库命中、某作者的命中直接丢失，
/// 总数也被截成窗口大小。来源与作者条件因此下推进 SQL，与 MATCH 同时生效
///
/// 为了让全量命中足够便宜，这里只投影 rowid、rank 与「主歌词列是否命中」，不读正文；
/// snippet 留空，由 [`fetch_fts_snippets`] 只对当前页的条目补取。
/// `field` 由 FTS 列过滤判定：主歌词列命中为 `MainLyric`，只有背景人声列命中为 `BackgroundVocal`
pub async fn search_fts(
    conn: &DatabaseConnection,
    keyword: &str,
    filter: &FtsFilter,
) -> Result<Vec<LyricHit>, AppError> {
    let Some(normalized) = normalize_fts_keyword(keyword) else {
        return Ok(Vec::new());
    };

    let safe_keyword = normalized.replace('"', "\"\"");
    let match_expr = format!("\"{safe_keyword}\"");
    let main_match_expr = format!("lyric_text : {match_expr}");

    let mut values: Vec<sea_orm::Value> = vec![match_expr.into(), main_match_expr.into()];
    let mut conditions: Vec<String> = Vec::new();
    if let Some(source) = filter.source {
        values.push(source.as_str().to_owned().into());
        conditions.push(format!("source = ${}", values.len()));
    }
    if let Some(ref author_id) = filter.author_id {
        values.push(author_id.clone().into());
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM json_each(lyrics.author_ids) WHERE json_each.value = ${})",
            values.len()
        ));
    }
    if let Some(ref author_username) = filter.author_username {
        values.push(author_username.clone().into());
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM json_each(lyrics.author_usernames) WHERE json_each.value = ${})",
            values.len()
        ));
    }
    let filter_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(
            "AND rowid IN (SELECT id FROM lyrics WHERE {})",
            conditions.join(" AND ")
        )
    };

    // `(0.0, 1.0, 0.25)` 对应 `(filename, lyric_text, bg_vocal_text)`；
    // `in_main` 的子查询与外层 MATCH 无关联，SQLite 只求值一次
    let sql = format!(
        r"
        SELECT
            rowid AS id,
            bm25(lyrics_fts, 0.0, 1.0, 0.25) AS rank,
            rowid IN (SELECT rowid FROM lyrics_fts WHERE lyrics_fts MATCH $2) AS in_main
        FROM lyrics_fts
        WHERE lyrics_fts MATCH $1
        {filter_clause}
        ORDER BY rank ASC, rowid ASC;
    "
    );

    let stmt = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values);

    let rows = conn
        .query_all_raw(stmt)
        .await
        .map_err(|e| AppError::InternalServerError(format!("FTS Query Error: {e}")))?;

    let mut hits = Vec::with_capacity(rows.len());
    for row in rows {
        let id: i64 = match row.try_get("", "id") {
            Ok(id) => id,
            Err(e) => {
                warn!("Failed to read 'id' column from FTS result: {e}");
                continue;
            }
        };

        let rank: f64 = match row.try_get("", "rank") {
            Ok(rank) => rank,
            Err(e) => {
                warn!("Failed to read 'rank' column from FTS result: {e}");
                continue;
            }
        };

        // 读不到时按主歌词处理，与此前「两列都抽不出 snippet 时记为主歌词」的兜底一致
        let in_main: i64 = row.try_get("", "in_main").unwrap_or(1);
        let field = if in_main == 0 {
            LyricMatchField::BackgroundVocal
        } else {
            LyricMatchField::MainLyric
        };

        hits.push(LyricHit {
            id: LyricId::from_u64_masked(id.cast_unsigned()),
            rank,
            field,
            snippet: None,
        });
    }

    Ok(hits)
}

/// 为一页 FTS 命中补取 snippet，返回 `id → snippet`；抽不出上下文的条目不出现在结果里
///
/// snippet 优先从主歌词提取，回退到背景人声。按 rowid 点查 FTS 表，只读这一页的正文
pub async fn fetch_fts_snippets(
    conn: &DatabaseConnection,
    keyword: &str,
    ids: &[LyricId],
) -> Result<HashMap<LyricId, String>, AppError> {
    let Some(normalized) = normalize_fts_keyword(keyword) else {
        return Ok(HashMap::new());
    };
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "SELECT rowid AS id, lyric_text, bg_vocal_text FROM lyrics_fts WHERE rowid IN ({});",
        placeholders.join(", ")
    );
    let values: Vec<sea_orm::Value> = ids.iter().map(|id| id.get().cast_signed().into()).collect();

    let stmt = Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values);
    let rows = conn
        .query_all_raw(stmt)
        .await
        .map_err(|e| AppError::InternalServerError(format!("FTS Snippet Query Error: {e}")))?;

    let mut snippets = HashMap::with_capacity(rows.len());
    for row in rows {
        let id: i64 = match row.try_get("", "id") {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("Failed to read 'id' column from FTS snippet result: {e}");
                continue;
            }
        };
        let lyric_text: String = row.try_get("", "lyric_text").unwrap_or_default();
        let bg_vocal_text: String = row.try_get("", "bg_vocal_text").unwrap_or_default();

        if let Some(snippet) = extract_lyric_context(&lyric_text, &normalized)
            .or_else(|| extract_lyric_context(&bg_vocal_text, &normalized))
        {
            snippets.insert(LyricId::from_u64_masked(id.cast_unsigned()), snippet);
        }
    }

    Ok(snippets)
}

#[cfg(test)]
#[path = "queries_tests.rs"]
mod tests;
