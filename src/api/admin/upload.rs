//! 私人歌词库上传接口：`POST /v1/admin/lyrics`
//!
//! 内部接口，不写进公开接口文档。用 `ADMIN_SECRET` 做 Bearer 鉴权；所有响应（含错误）
//! 一律 `no-store`，带着 token 的内容不该留在任何缓存里

use axum::{
    extract::{
        Multipart,
        State,
    },
    http::{
        HeaderMap,
        header::{
            AUTHORIZATION,
            CACHE_CONTROL,
        },
    },
    response::{
        IntoResponse,
        Response,
    },
};
use sea_orm::{
    DatabaseConnection,
    EntityTrait,
};
use serde::Serialize;
use sha2::{
    Digest,
    Sha256,
};
use subtle::ConstantTimeEq;
use tracing::error;

use crate::{
    api::shared::{
        cache::NO_STORE_CACHE_CONTROL,
        dto::ApiSuccess,
    },
    core::{
        LyricId,
        db::entity,
        error::AppError,
    },
    services::{
        AppState,
        sync_service::build_entity_from_ttml,
    },
};

/// `POST /v1/admin/lyrics`：上传一首私库 TTML 歌词，`source='private'`
pub async fn handle_upload_lyric(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    no_store(
        upload_lyric(&state, headers, multipart)
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
}

async fn upload_lyric(
    state: &AppState,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<ApiSuccess<UploadLyricData>, AppError> {
    authorize_admin(state, &headers)?;

    let mut filename_raw: Option<String> = None;
    let mut file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Invalid multipart body: {e}")))?
    {
        match field.name() {
            Some("filename") => {
                filename_raw =
                    Some(field.text().await.map_err(|e| {
                        AppError::BadRequest(format!("Invalid filename field: {e}"))
                    })?);
            }
            Some("file") => {
                file_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("Invalid file field: {e}")))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let filename_raw =
        filename_raw.ok_or_else(|| AppError::BadRequest("Missing 'filename' field".to_string()))?;
    let file_bytes =
        file_bytes.ok_or_else(|| AppError::BadRequest("Missing 'file' field".to_string()))?;

    let filename = validate_filename(&filename_raw)?;
    let content = String::from_utf8(file_bytes)
        .map_err(|_| AppError::BadRequest("file is not valid UTF-8".to_string()))?;

    let id = upsert_private_lyric(&state.store.conn, &filename, &content).await?;

    // 上传后立即可查是本接口的契约：索引重建失败就大声报错，
    // 不像后台同步那样 warn 后继续用旧索引
    match state.store.rebuild_index().await {
        Ok(()) => state.store.invalidate_caches(),
        Err(e) => {
            error!("upload index rebuild failed: {e:?}");
            return Err(AppError::InternalServerError(
                "Lyric saved but index rebuild failed".to_string(),
            ));
        }
    }

    Ok(ApiSuccess(UploadLyricData {
        id: id.get(),
        filename,
        source: "private",
    }))
}

/// 解析 TTML 并以 `source='private'` upsert；同名私库文件再次上传会整行更新
async fn upsert_private_lyric(
    conn: &DatabaseConnection,
    filename: &str,
    content: &str,
) -> Result<LyricId, AppError> {
    let result = ttml_processor::parse_ttml(content).map_err(|e| {
        let mut msg = format!("{e:?}");
        if msg.len() > 300 {
            msg.truncate(300);
        }
        AppError::BadRequest(format!("Invalid TTML: {msg}"))
    })?;

    let mut model = build_entity_from_ttml(filename, content, &result);
    model.source = "private".to_string();
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
            error!("private lyric upsert failed: {e:?}");
            if e.to_string().to_lowercase().contains("unique") {
                AppError::BadRequest("filename already exists in the public library".to_string())
            } else {
                AppError::InternalServerError("Failed to save lyric".to_string())
            }
        })?;

    Ok(LyricId::from_private_filename(filename))
}

fn validate_filename(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "filename must not be empty".to_string(),
        ));
    }
    if name.len() > 255 {
        return Err(AppError::BadRequest(
            "filename too long (max 255 bytes)".to_string(),
        ));
    }
    if !name.to_lowercase().ends_with(".ttml") {
        return Err(AppError::BadRequest(
            "filename must end with .ttml".to_string(),
        ));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(AppError::BadRequest(
            "filename must not contain path separators".to_string(),
        ));
    }
    Ok(name.to_string())
}

/// 未配置密钥是服务端配置错误；Bearer 缺失或不匹配是 401
fn authorize_admin(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let secret = state.admin_secret.as_ref().ok_or_else(|| {
        AppError::InternalServerError(
            "ADMIN_SECRET environment variable is not configured on the server.".to_string(),
        )
    })?;

    let provided = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;

    if token_matches(provided, secret.as_str()) {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
}

/// 常量时间比较。先各自取哈希，长度差异也不会从耗时上泄露
fn token_matches(provided: &str, expected: &str) -> bool {
    let provided = Sha256::digest(provided.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());
    provided.as_slice().ct_eq(expected.as_slice()).into()
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, NO_STORE_CACHE_CONTROL);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        db::{
            queries::fetch_song_entries,
            setup::init_db,
        },
        models::LyricSource,
    };

    const SAMPLE_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:00:01.000" end="00:00:02.000">hello</p></div></body></tt>"#;

    #[test]
    fn validate_filename_accepts_ttml() {
        assert_eq!(
            validate_filename("Artist - Title.ttml").unwrap(),
            "Artist - Title.ttml"
        );
        assert_eq!(validate_filename("  a.TTML  ").unwrap(), "a.TTML");
    }

    #[test]
    fn validate_filename_rejects_bad_inputs() {
        for bad in [
            "",
            "   ",
            "song.lrc",
            "../evil.ttml",
            "a/b.ttml",
            "a\\b.ttml",
            &"x".repeat(300),
        ] {
            assert!(validate_filename(bad).is_err(), "should reject {bad:?}");
        }
    }

    fn bearer_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {token}").parse().expect("valid header"),
        );
        headers
    }

    #[tokio::test]
    async fn authorize_admin_rejects_unconfigured_secret() {
        let conn = init_db("sqlite::memory:").await.expect("init db");
        let state = AppState::new_with_secret(conn, None);

        let err = authorize_admin(&state, &bearer_headers("s3cret")).unwrap_err();
        assert!(matches!(err, AppError::InternalServerError(_)));
    }

    #[tokio::test]
    async fn authorize_admin_checks_bearer() {
        let conn = init_db("sqlite::memory:").await.expect("init db");
        let state = AppState::new_with_secret(conn, None).with_admin_secret("s3cret");

        assert!(authorize_admin(&state, &bearer_headers("s3cret")).is_ok());
        assert!(matches!(
            authorize_admin(&state, &bearer_headers("wrong")).unwrap_err(),
            AppError::Unauthorized
        ));
        assert!(matches!(
            authorize_admin(&state, &HeaderMap::new()).unwrap_err(),
            AppError::Unauthorized
        ));
    }

    #[tokio::test]
    async fn upsert_private_lyric_round_trip() {
        let conn = init_db("sqlite::memory:").await.expect("init db");

        let id1 = upsert_private_lyric(&conn, "mine.ttml", SAMPLE_TTML)
            .await
            .expect("first upload");
        assert_eq!(id1, LyricId::from_private_filename("mine.ttml"));

        let updated = SAMPLE_TTML.replace("hello", "hello updated");
        let id2 = upsert_private_lyric(&conn, "mine.ttml", &updated)
            .await
            .expect("second upload");
        assert_eq!(id1, id2, "re-upload keeps the same id");

        let entries = fetch_song_entries(&conn).await.expect("fetch entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, LyricSource::Private);
        assert_eq!(entries[0].id, id1);

        // lyric_text 应为更新后的内容
        let row = entity::Entity::find_by_id(id1.get().cast_signed())
            .one(&conn)
            .await
            .expect("query")
            .expect("row exists");
        assert!(row.lyric_text.contains("hello updated"));
    }
}
