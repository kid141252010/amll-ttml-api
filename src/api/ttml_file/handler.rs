//! SPlayer-Next 兼容的 TTML 文件直链接口。
//!
//! 对标官方 AMLL TTML DB 的 URL 模板 `{host}/%p/%s.ttml`：
//! `%p` 是平台目录名（SPlayer-Next 源码实测：NCM → "ncm-lyrics"，QM → "qq-lyrics"），
//! `%s` 是平台歌曲 ID。`GET /{platform}/{song_id}.ttml` 按平台 ID 查库，
//! 私库优先，直接返回 TTML 原文。

use axum::{
    extract::{
        Path,
        State,
    },
    http::header,
    response::IntoResponse,
};

use crate::{
    core::{
        error::AppError,
        models::IdQuery,
    },
    services::{
        AppState,
        lyric_service,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlatformKind {
    Ncm,
    Qq,
    Am,
    Spotify,
}

fn resolve_platform(platform: &str) -> Result<PlatformKind, AppError> {
    match platform {
        // 平台目录名与官方 amll-ttml-db 仓库一致（SPlayer-Next 只会用到前两种）
        "ncm-lyrics" => Ok(PlatformKind::Ncm),
        "qq-lyrics" => Ok(PlatformKind::Qq),
        "am-lyrics" => Ok(PlatformKind::Am),
        "spotify-lyrics" => Ok(PlatformKind::Spotify),
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
        PlatformKind::Am => id_query.apple_music_ids.push(song_id.to_owned()),
        PlatformKind::Spotify => id_query.spotify_ids.push(song_id.to_owned()),
    }

    let (_, ttml_text) = lyric_service::get_lyric(&state.store, id_query).await?;

    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        ttml_text,
    ))
}

#[cfg(test)]
mod tests;
