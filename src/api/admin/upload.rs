//! 私人歌词库上传接口：`POST /v1/admin/lyrics` 及 `/v1/admin/lyrics/batch`
//!
//! 内部接口，不写进公开接口文档。用 `ADMIN_SECRET` 做 Bearer 鉴权；所有响应（含错误）
//! 一律 `no-store`，带着 token 的内容不该留在任何缓存里

use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use axum::{
    extract::{
        Multipart,
        State,
        multipart::MultipartRejection,
    },
    http::HeaderMap,
    response::{
        IntoResponse,
        Response,
    },
};
use sea_orm::{
    DatabaseConnection,
    DbErr,
    EntityTrait,
    SqlErr,
};
use serde::Serialize;
use tracing::error;

use crate::{
    api::shared::{
        auth::{
            authorize_bearer,
            no_store,
        },
        dto::ApiSuccess,
        query::has_ttml_extension,
    },
    core::{
        LyricId,
        db::entity,
        error::AppError,
    },
    services::{
        AppState,
        sync_service::{
            build_entity_from_ttml,
            merge_ttml_platform_ids,
        },
    },
};

/// `POST /v1/admin/lyrics`：上传一首私库 TTML 歌词，`source='private'`
pub async fn handle_upload_lyric(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    no_store(
        upload_lyric(&state, &headers, multipart)
            .await
            .into_response(),
    )
}

/// `POST /v1/admin/lyrics/batch`：顺序上传多首私库歌词，全部处理后统一重建索引
pub async fn handle_upload_lyrics_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    no_store(
        upload_lyrics_batch(&state, &headers, multipart)
            .await
            .into_response(),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadLyricData {
    pub id: u64,
    pub filename: String,
    pub source: &'static str,
    /// 条目的第一歌名，管理页用它生成搜索确认链接；TTML 没有歌名时为 `null`
    pub music_name: Option<String>,
}

const MAX_BATCH_FILES: usize = 100;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadLyricsBatchData {
    succeeded: usize,
    failed: usize,
    results: Vec<UploadLyricResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadLyricResult {
    filename: String,
    id: Option<u64>,
    music_name: Option<String>,
    error: Option<String>,
}

async fn upload_lyrics_batch(
    state: &AppState,
    headers: &HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<ApiSuccess<UploadLyricsBatchData>, AppError> {
    authorize_admin(state, headers)?;
    let mut multipart =
        multipart.map_err(|e| AppError::BadRequest(format!("Invalid multipart body: {e}.")))?;
    let mut files = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Invalid multipart body: {e}.")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        if files.len() == MAX_BATCH_FILES {
            return Err(AppError::BadRequest(format!(
                "Too many files (max {MAX_BATCH_FILES})."
            )));
        }
        let filename = field.file_name().unwrap_or_default().to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("Invalid file field: {e}.")))?;
        files.push((filename, bytes.to_vec()));
    }

    process_batch_upload(state, files).await.map(ApiSuccess)
}

/// 先检查整批数量再写库，单个文件失败不影响后续文件
async fn process_batch_upload(
    state: &AppState,
    files: Vec<(String, Vec<u8>)>,
) -> Result<UploadLyricsBatchData, AppError> {
    if files.is_empty() {
        return Err(AppError::BadRequest("Missing 'file' field.".to_string()));
    }
    if files.len() > MAX_BATCH_FILES {
        return Err(AppError::BadRequest(format!(
            "Too many files (max {MAX_BATCH_FILES})."
        )));
    }

    let mut succeeded = 0;
    let mut results = Vec::with_capacity(files.len());
    for (filename, bytes) in files {
        let result = async {
            let filename = validate_filename(&filename)?;
            let content = std::str::from_utf8(&bytes)
                .map_err(|_| AppError::BadRequest("File is not valid UTF-8.".to_string()))?;
            upsert_private_lyric(&state.store.conn, &filename, content).await
        }
        .await;

        let (id, music_name, error) = match result {
            Ok(saved) => {
                succeeded += 1;
                (Some(saved.id.get()), saved.music_name, None)
            }
            Err(error) => {
                let message = match error {
                    AppError::BadRequest(message) | AppError::InternalServerError(message) => {
                        message
                    }
                    _ => "Failed to save lyric.".to_string(),
                };
                (None, None, Some(message))
            }
        };
        results.push(UploadLyricResult {
            filename,
            id,
            music_name,
            error,
        });
    }

    // 整批写完后只重建一次；全部失败时保留现有索引与热缓存
    if succeeded > 0 {
        match state.store.rebuild_index().await {
            Ok(()) => state.store.invalidate_caches(),
            Err(e) => {
                error!("batch upload index rebuild failed: {e:?}");
                return Err(AppError::InternalServerError(
                    "Lyric saved but index rebuild failed.".to_string(),
                ));
            }
        }
    }

    Ok(UploadLyricsBatchData {
        succeeded,
        failed: results.len() - succeeded,
        results,
    })
}

async fn upload_lyric(
    state: &AppState,
    headers: &HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<ApiSuccess<UploadLyricData>, AppError> {
    authorize_admin(state, headers)?;
    let mut multipart =
        multipart.map_err(|e| AppError::BadRequest(format!("Invalid multipart body: {e}")))?;

    let mut filename_raw: Option<String> = None;
    let mut file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Invalid multipart body: {e}.")))?
    {
        match field.name() {
            Some("filename") => {
                filename_raw =
                    Some(field.text().await.map_err(|e| {
                        AppError::BadRequest(format!("Invalid filename field: {e}."))
                    })?);
            }
            Some("file") => {
                file_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("Invalid file field: {e}.")))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let filename_raw = filename_raw
        .ok_or_else(|| AppError::BadRequest("Missing 'filename' field.".to_string()))?;
    let file_bytes =
        file_bytes.ok_or_else(|| AppError::BadRequest("Missing 'file' field.".to_string()))?;

    let filename = validate_filename(&filename_raw)?;
    let content = String::from_utf8(file_bytes)
        .map_err(|_| AppError::BadRequest("File is not valid UTF-8.".to_string()))?;

    let saved = upsert_private_lyric(&state.store.conn, &filename, &content).await?;

    // 上传后立即可查是本接口的契约：索引重建失败就大声报错，
    // 不像后台同步那样 warn 后继续用旧索引
    match state.store.rebuild_index().await {
        Ok(()) => state.store.invalidate_caches(),
        Err(e) => {
            error!("upload index rebuild failed: {e:?}");
            return Err(AppError::InternalServerError(
                "Lyric saved but index rebuild failed.".to_string(),
            ));
        }
    }

    Ok(ApiSuccess(UploadLyricData {
        id: saved.id.get(),
        filename,
        source: "private",
        music_name: saved.music_name,
    }))
}

#[derive(Debug, PartialEq, Eq)]
struct SavedLyric {
    id: LyricId,
    /// 第一个非空歌名，与搜索页卡片标题取 `musicNames[0]` 的口径一致
    music_name: Option<String>,
}

/// 解析 TTML 并以 `source='private'` upsert；同名私库文件再次上传会整行更新
async fn upsert_private_lyric(
    conn: &DatabaseConnection,
    filename: &str,
    content: &str,
) -> Result<SavedLyric, AppError> {
    let result = ttml_processor::parse_ttml(content).map_err(|e| {
        let mut msg = format!("{e:?}");
        if msg.len() > 300 {
            msg.truncate(300);
        }
        AppError::BadRequest(format!("Invalid TTML: {msg}."))
    })?;

    let music_name = result
        .metadata
        .title
        .iter()
        .flatten()
        .map(|name| name.trim())
        .find(|name| !name.is_empty())
        .map(str::to_string);

    let mut model = build_entity_from_ttml(filename, content, &result);
    merge_ttml_platform_ids(&mut model, &result.metadata);
    model.source = "private".to_string();
    // 私库文件名不带公库那样的时间戳前缀，按文件名派生会兜底成 0，
    // 在 `/lyrics/list` 默认排序里沉底、被 `since` 过滤掉。改用上传时刻；
    // 同名重传会刷新它，让 `since` 增量拉取能看到这次更新
    model.timestamp = now_millis();
    model.id = LyricId::from_private_filename(filename).get().cast_signed();

    let active: entity::ActiveModel = model.into();
    entity::Entity::insert(active)
        .on_conflict(
            sea_orm::sea_query::OnConflict::columns([
                entity::Column::Source,
                entity::Column::Filename,
            ])
            .update_columns([
                entity::Column::Source,
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
                entity::Column::LyricText,
                entity::Column::BgVocalText,
                entity::Column::RawTtml,
            ])
            .to_owned(),
        )
        .exec(conn)
        .await
        .map_err(|e| {
            if is_public_filename_conflict(&e) {
                AppError::BadRequest("Filename already exists in the public library.".to_string())
            } else {
                error!("private lyric upsert failed: {e:?}");
                AppError::InternalServerError("Failed to save lyric.".to_string())
            }
        })?;

    Ok(SavedLyric {
        id: LyricId::from_private_filename(filename),
        music_name,
    })
}

/// upsert 失败是否因为公库已有同名文件
///
/// 私库行自身的冲突键是 `(source, filename)`，已由 `ON CONFLICT` 转成更新；
/// 能漏到这里的唯一约束只有 `lyrics.filename` 列级 `UNIQUE`（与公库同名）与主键。
/// SQLite 不报约束名，只能从结构化的 [`SqlErr::UniqueConstraintViolation`] 里取出
/// 驱动消息，再按 SQLite 固定的 `UNIQUE constraint failed: <表>.<列>` 格式认列名，
/// 把主键冲突等其他情况留给 500
fn is_public_filename_conflict(err: &DbErr) -> bool {
    matches!(
        err.sql_err(),
        Some(SqlErr::UniqueConstraintViolation(msg))
            if msg == "UNIQUE constraint failed: lyrics.filename"
    )
}

fn validate_filename(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "Filename must not be empty.".to_string(),
        ));
    }
    if name.len() > 255 {
        return Err(AppError::BadRequest(
            "Filename too long (max 255 bytes).".to_string(),
        ));
    }
    if !has_ttml_extension(name) {
        return Err(AppError::BadRequest(
            "Filename must end with '.ttml'.".to_string(),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(AppError::BadRequest(
            "Filename must not contain path separators.".to_string(),
        ));
    }
    Ok(name.to_string())
}

/// 当前时刻的 Unix epoch 毫秒，作为私库条目的 `createdAt`
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// 未配置密钥是服务端配置错误；Bearer 缺失或不匹配是 401
fn authorize_admin(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    authorize_bearer(headers, state.admin_secret.as_deref(), "ADMIN_SECRET")
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;
