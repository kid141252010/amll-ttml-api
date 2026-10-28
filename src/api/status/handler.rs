use axum::{
    Json,
    extract::State,
    http::header,
    response::IntoResponse,
};
use serde::Serialize;

use crate::{
    api::shared::cache::NO_STORE_CACHE_CONTROL,
    services::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub commit: &'static str,
    pub commit_date: &'static str,
    pub build_time: &'static str,
    pub rustc_version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub uptime_seconds: u64,
    pub lyric_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub build: BuildInfo,
    pub runtime: RuntimeStatus,
}

pub async fn handle_status(State(state): State<AppState>) -> impl IntoResponse {
    let uptime_seconds = state.start_time.elapsed().as_secs();
    let lyric_count = state.store.lyric_count();

    let response = StatusResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        build: BuildInfo {
            commit: env!("GIT_HASH"),
            commit_date: env!("GIT_COMMIT_DATE"),
            build_time: env!("BUILD_TIME"),
            rustc_version: env!("RUSTC_VERSION"),
        },
        runtime: RuntimeStatus {
            uptime_seconds,
            lyric_count,
        },
    };

    (
        [(header::CACHE_CONTROL, NO_STORE_CACHE_CONTROL)],
        Json(response),
    )
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
