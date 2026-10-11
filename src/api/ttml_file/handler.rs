//! SPlayer-Next 兼容的 TTML 文件直链接口。
//!
//! 对标官方 AMLL TTML DB 的 URL 模板 `{host}/%p/%s.ttml`：
//! `GET /{platform}/{song_id}.ttml` 按平台 ID 查库，私库优先，直接返回 TTML 原文。

use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
};

use crate::{
    core::{error::AppError, models::IdQuery},
    services::{AppState, lyric_service},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlatformKind {
    Ncm,
    Qq,
    Apple,
    Spotify,
}

fn resolve_platform(platform: &str) -> Result<PlatformKind, AppError> {
    match platform {
        "ncm" => Ok(PlatformKind::Ncm),
        "qq" => Ok(PlatformKind::Qq),
        "apple" => Ok(PlatformKind::Apple),
        "spotify" => Ok(PlatformKind::Spotify),
        _ => Err(AppError::BadRequest(format!(
            "unsupported platform: {platform}"
        ))),
    }
}

fn strip_ttml_suffix(file: &str) -> Option<&str> {
    file.strip_suffix(".ttml")
}

pub async fn handle_ttml_file(
    State(state): State<AppState>,
    Path((platform, file)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    // 注意检查顺序：`/{platform}/{file}` 是两段捕获路由，也会匹配到
    // `/v1/<未知子路径>` 这类原本走嵌套 fallback 返回 404 的请求。
    // 先判后缀：非 .ttml 请求一律 404，保持现有路由行为不变；
    // 再判 platform：未知平台 + .ttml 后缀才 400。
    let song_id = strip_ttml_suffix(&file).ok_or(AppError::NotFound)?;
    let platform = resolve_platform(&platform)?;

    let mut id_query = IdQuery::default();
    match platform {
        PlatformKind::Ncm => id_query.ncm_music_ids.push(song_id.to_owned()),
        PlatformKind::Qq => id_query.qq_music_ids.push(song_id.to_owned()),
        PlatformKind::Apple => id_query.apple_music_ids.push(song_id.to_owned()),
        PlatformKind::Spotify => id_query.spotify_ids.push(song_id.to_owned()),
    }

    let (_, ttml_text) = lyric_service::get_lyric(&state.store, id_query).await?;

    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        ttml_text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_supported_platforms() {
        assert!(matches!(resolve_platform("ncm"), Ok(PlatformKind::Ncm)));
        assert!(matches!(resolve_platform("qq"), Ok(PlatformKind::Qq)));
        assert!(matches!(resolve_platform("apple"), Ok(PlatformKind::Apple)));
        assert!(matches!(resolve_platform("spotify"), Ok(PlatformKind::Spotify)));
    }

    #[test]
    fn rejects_unknown_platform() {
        assert!(resolve_platform("kugou").is_err());
        assert!(resolve_platform("").is_err());
    }

    #[test]
    fn strips_ttml_suffix() {
        assert_eq!(strip_ttml_suffix("123.ttml"), Some("123"));
        assert_eq!(strip_ttml_suffix("abc123.ttml"), Some("abc123"));
    }

    #[test]
    fn rejects_non_ttml_files() {
        assert_eq!(strip_ttml_suffix("123.xml"), None);
        assert_eq!(strip_ttml_suffix("123.ttm"), None);
        assert_eq!(strip_ttml_suffix("123.TTML"), None);
        assert_eq!(strip_ttml_suffix("123"), None);
    }
}
