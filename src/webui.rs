use axum::{
    http::header::CONTENT_TYPE,
    response::{
        IntoResponse,
        Response,
    },
};
use rust_embed::RustEmbed;

use crate::core::error::AppError;

#[derive(RustEmbed)]
#[folder = "web/"]
struct WebAssets;

pub async fn serve_index() -> Result<impl IntoResponse, AppError> {
    serve_file("index.html")
}

pub async fn serve_admin() -> Result<impl IntoResponse, AppError> {
    serve_file("admin.html")
}

// 静态资源用显式路由逐个注册：`/{*path}` 通配符会与 `/{platform}/{file}`
//（SPlayer-Next TTML 直链）在 Axum 0.8 的路由表里冲突导致建表 panic。
pub async fn serve_app_js() -> Result<impl IntoResponse, AppError> {
    serve_file("app.js")
}

pub async fn serve_style_css() -> Result<impl IntoResponse, AppError> {
    serve_file("style.css")
}

fn serve_file(path: &str) -> Result<Response, AppError> {
    let asset = WebAssets::get(path).ok_or(AppError::NotFound)?;
    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    Ok(([(CONTENT_TYPE, content_type)], asset.data).into_response())
}
