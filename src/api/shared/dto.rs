use axum::{
    Json,
    http::{
        StatusCode,
        header::CACHE_CONTROL,
    },
    response::{
        IntoResponse,
        Response,
    },
};
use compact_str::CompactString;
use serde::{
    Serialize,
    Serializer,
};

use crate::{
    api::shared::cache::NOT_FOUND_CACHE_CONTROL,
    core::{
        LyricId,
        error::AppError,
        models::{
            LyricSource,
            SongEntry,
        },
        pagination::PaginationInfo,
    },
};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SongItem {
    pub id: LyricId,
    pub filename: String,
    /// 本条歌词条目的创建时刻，Unix epoch 毫秒
    ///
    /// 由上游 CI 处理投稿时生成并编码进文件名前缀，同一文件恒定。
    /// 它既不是合并进词库的时刻（两者可能相隔数月），也不是歌曲本身的发行日期
    pub created_at: u64,

    /// 条目来源：公库（上游同步）或私库（用户上传）
    pub source: LyricSource,

    pub music_names: Box<[CompactString]>,
    pub artist_names: Box<[CompactString]>,
    pub album_names: Box<[CompactString]>,

    pub ncm_music_ids: Box<[CompactString]>,
    pub qq_music_ids: Box<[CompactString]>,
    pub apple_music_ids: Box<[CompactString]>,
    pub spotify_ids: Box<[CompactString]>,

    pub isrcs: Box<[CompactString]>,

    pub author_ids: Box<[CompactString]>,
    pub author_usernames: Box<[CompactString]>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_context: Option<MatchContext>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MatchContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchData {
    pub items: Vec<SongItem>,
    pub pagination: PaginationInfo,
}

pub struct HttpOk;

impl Serialize for HttpOk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u16(200)
    }
}

#[derive(Serialize)]
pub struct SuccessResponse<T> {
    pub status: HttpOk,
    pub data: T,
}

pub struct ApiSuccess<T>(pub T);

impl<T: Serialize> IntoResponse for ApiSuccess<T> {
    fn into_response(self) -> Response {
        let envelope = SuccessResponse {
            status: HttpOk,
            data: self.0,
        };
        Json(envelope).into_response()
    }
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub status: u16,
    pub error: &'static str,
    pub message: String,
}

const INTERNAL_ERROR_MESSAGE: &str = "An internal error occurred.";
const UPSTREAM_ERROR_MESSAGE: &str = "Upstream service unavailable.";

fn error_parts(err: &AppError) -> (u16, &'static str, String) {
    match err {
        AppError::ReqwestError(_) | AppError::JsonError(_) | AppError::InternalServerError(_) => {
            (500, "Internal Server Error", INTERNAL_ERROR_MESSAGE.into())
        }
        AppError::NotFound => (
            404,
            "Not Found",
            "The requested API route does not exist.".into(),
        ),
        AppError::LyricNotFound => (
            404,
            "Not Found",
            "No lyrics found for the provided query.".into(),
        ),
        AppError::BadRequest(msg) => (400, "Bad Request", msg.clone()),
        AppError::Unauthorized => (
            401,
            "Unauthorized",
            "Invalid or missing authorization token.".into(),
        ),
        AppError::TooEarly => (
            425,
            "Too Early",
            "Request arrived in TLS early data and may be a replay; retry on a completed handshake."
                .into(),
        ),
        AppError::UpstreamError(_) => (502, "Bad Gateway", UPSTREAM_ERROR_MESSAGE.into()),
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_str, message) = error_parts(&self);

        if status >= 500 {
            tracing::error!(
                status,
                error = error_str,
                detail = ?self,
                "Request failed with server error"
            );
        }

        let payload = ErrorResponse {
            status,
            error: error_str,
            message,
        };

        let mut response = (
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(payload),
        )
            .into_response();

        if status == 404 {
            response
                .headers_mut()
                .insert(CACHE_CONTROL, NOT_FOUND_CACHE_CONTROL);
        }

        response
    }
}

pub fn map_song_to_item(
    song: &SongEntry,
    lyrics: Option<String>,
    format: Option<String>,
    match_context: Option<MatchContext>,
) -> SongItem {
    SongItem {
        id: song.id,
        filename: song.filename.to_string(),
        created_at: song.timestamp,
        source: song.source,
        music_names: song.track_names.clone(),
        artist_names: song.artist_names.clone(),
        album_names: song.album_names.clone(),

        ncm_music_ids: song.ncm_music_ids.clone(),
        qq_music_ids: song.qq_music_ids.clone(),
        apple_music_ids: song.apple_music_ids.clone(),
        spotify_ids: song.spotify_ids.clone(),

        isrcs: song.isrcs.clone(),

        author_ids: song.author_ids.clone(),
        author_usernames: song.author_usernames.clone(),

        lyrics,
        format,

        match_context,
    }
}

#[cfg(test)]
mod tests;
