//! 内部运维端点：团队下载请求统计的每日 Parquet 文件
//!
//! 不写进公开接口文档。用 `ANALYTICS_SECRET` 做 Bearer 鉴权；所有响应（含错误）一律 `no-store`，
//! 带着 token 的内容不该留在任何缓存里

use axum::{
    body::Body,
    extract::{
        Path,
        Request,
        State,
    },
    http::{
        HeaderMap,
        HeaderValue,
        header::{
            CONTENT_DISPOSITION,
            CONTENT_TYPE,
        },
    },
    response::{
        IntoResponse,
        Response,
    },
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{
    analytics::Analytics,
    api::{
        admin::dto::{
            AnalyticsFilesData,
            map_daily_file,
        },
        shared::{
            auth::{
                authorize_bearer,
                no_store,
            },
            dto::ApiSuccess,
        },
    },
    core::error::AppError,
    services::AppState,
};

const PARQUET_CONTENT_TYPE: HeaderValue =
    HeaderValue::from_static("application/vnd.apache.parquet");

/// 让 nginx 边读边发，而不是先把整个文件缓冲到自己的临时目录（会占服务器磁盘）
const ACCEL_BUFFERING: &str = "x-accel-buffering";

/// `GET /v1/admin/analytics/files`：可供下载的每日文件清单
pub async fn handle_list_files(State(state): State<AppState>, headers: HeaderMap) -> Response {
    no_store(list_files(&state, &headers).await.into_response())
}

/// `GET /v1/admin/analytics/files/{name}`：下载一个每日文件，支持 Range 断点续传
pub async fn handle_download_file(
    State(state): State<AppState>,
    Path(name): Path<String>,
    request: Request,
) -> Response {
    no_store(download_file(&state, &name, request).await.into_response())
}

async fn list_files(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ApiSuccess<AnalyticsFilesData>, AppError> {
    let analytics = authorize(state, headers)?;

    let files = analytics.daily_files().await.map_err(|e| {
        AppError::InternalServerError(format!("Failed to list analytics files: {e}"))
    })?;

    Ok(ApiSuccess(AnalyticsFilesData {
        files: files.into_iter().map(map_daily_file).collect(),
    }))
}

async fn download_file(
    state: &AppState,
    name: &str,
    request: Request,
) -> Result<Response, AppError> {
    let analytics = authorize(state, request.headers())?;

    let path = analytics.daily_file_path(name).ok_or(AppError::NotFound)?;
    // ServeFile 对缺失文件返回自己的空 404，先查一遍，保持统一的错误形状
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Err(AppError::NotFound);
    }

    let response = ServeFile::new(&path)
        .oneshot(request)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to serve {name}: {e}")))?;

    let mut response = response.map(Body::new);
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, PARQUET_CONTENT_TYPE);
    if let Ok(disposition) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(CONTENT_DISPOSITION, disposition);
    }
    headers.insert(ACCEL_BUFFERING, HeaderValue::from_static("no"));

    Ok(response)
}

/// 统计未启用时端点等同于不存在；启用了但没配密钥是服务端配置错误
fn authorize<'a>(state: &'a AppState, headers: &HeaderMap) -> Result<&'a Analytics, AppError> {
    let analytics = state.analytics.as_ref().ok_or(AppError::NotFound)?;
    authorize_bearer(headers, analytics.secret(), "ANALYTICS_SECRET")?;
    Ok(analytics)
}
