use axum::{
    extract::Path,
    http::header::CONTENT_TYPE,
    response::{IntoResponse, Response},
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

pub async fn serve_asset(Path(path): Path<String>) -> Result<impl IntoResponse, AppError> {
    serve_file(&path)
}

fn serve_file(path: &str) -> Result<Response, AppError> {
    let asset = WebAssets::get(path).ok_or(AppError::NotFound)?;
    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    Ok(([(CONTENT_TYPE, content_type)], asset.data).into_response())
}
