//! SQLite 查询层
//!
//! 负责 FTS 全文检索、读取原始 TTML 文件、读取重建内存索引所需的元数据条目
//!
//! 查询与 `setup.rs` 中的 FTS DDL（列序、trigram tokenizer、同步触发器）必须保持同步，
//! 修改任何一边必须要同步另一边

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

use crate::{
    core::{
        LyricId,
        db::entity,
        error::AppError,
        matcher::convert_tw2s,
        models::{
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

/// FTS5 全文检索歌词正文，返回按 bm25 相关度排序的命中
///
/// 关键词先做繁简转换与小写化，双引号转义后以短语形式传给 MATCH；
/// snippet 优先从主歌词提取，回退到背景人声
pub async fn search_fts(
    conn: &DatabaseConnection,
    keyword: &str,
    limit: u64,
) -> Result<Vec<LyricHit>, AppError> {
    if keyword.trim().is_empty() {
        return Ok(Vec::new());
    }

    let normalized = convert_tw2s(keyword).to_lowercase();
    let safe_keyword = normalized.replace('"', "\"\"");
    let match_expr = format!("\"{safe_keyword}\"");

    // `(0.0, 1.0, 0.25)` 对应 `(filename, lyric_text, bg_vocal_text)`
    let sql = r"
        SELECT
            rowid AS id,
            bm25(lyrics_fts, 0.0, 1.0, 0.25) AS rank,
            lyric_text,
            bg_vocal_text
        FROM lyrics_fts
        WHERE lyrics_fts MATCH $1
        ORDER BY rank ASC
        LIMIT $2;
    ";

    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        vec![match_expr.into(), limit.into()],
    );

    let rows = conn
        .query_all_raw(stmt)
        .await
        .map_err(|e| AppError::InternalServerError(format!("FTS Query Error: {e}")))?;

    let mut hits = Vec::with_capacity(rows.len());
    for row in rows {
        let id: i64 = match row.try_get("", "id") {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("Failed to read 'id' column from FTS result: {e}");
                continue;
            }
        };

        let rank: f64 = match row.try_get("", "rank") {
            Ok(rank) => rank,
            Err(e) => {
                tracing::warn!("Failed to read 'rank' column from FTS result: {e}");
                continue;
            }
        };

        let lyric_text: String = row.try_get("", "lyric_text").unwrap_or_default();
        let bg_vocal_text: String = row.try_get("", "bg_vocal_text").unwrap_or_default();

        let (field, snippet) = extract_lyric_context(&lyric_text, &normalized)
            .map(|s| (LyricMatchField::MainLyric, Some(s)))
            .or_else(|| {
                extract_lyric_context(&bg_vocal_text, &normalized)
                    .map(|s| (LyricMatchField::BackgroundVocal, Some(s)))
            })
            .unwrap_or((LyricMatchField::MainLyric, None));

        hits.push(LyricHit {
            id: LyricId::from_u64_masked(id.cast_unsigned()),
            rank,
            field,
            snippet,
        });
    }

    Ok(hits)
}

#[cfg(test)]
mod tests;
