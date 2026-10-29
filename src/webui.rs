use axum::{
    http::header::{
        CONTENT_SECURITY_POLICY,
        CONTENT_TYPE,
        X_CONTENT_TYPE_OPTIONS,
        X_FRAME_OPTIONS,
    },
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

// 应用自身附带基础防护头，不依赖 nginx：Docker 直接暴露端口的部署同样生效。
// 两个页面只加载同源的 /app.js 与 /style.css，没有内联样式、data: 图片或外部资源；
// 搜索页的结果卡片拼的是上游元数据，脚本只放行同源文件，挡住 innerHTML 注入。
// 管理页有一段内联 <script>，只能额外放行 'unsafe-inline'
const INDEX_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
                         base-uri 'self'; form-action 'self'; frame-ancestors 'none'";
const ADMIN_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
                         style-src 'self'; base-uri 'self'; form-action 'self'; \
                         frame-ancestors 'none'";

pub async fn serve_index() -> Result<impl IntoResponse, AppError> {
    serve_page("index.html", INDEX_CSP)
}

pub async fn serve_admin() -> Result<impl IntoResponse, AppError> {
    serve_page("admin.html", ADMIN_CSP)
}

// 静态资源用显式路由逐个注册：`/{*path}` 通配符会与 `/{platform}/{file}`
//（SPlayer-Next TTML 直链）在 Axum 0.8 的路由表里冲突导致建表 panic。
pub async fn serve_app_js() -> Result<impl IntoResponse, AppError> {
    serve_file("app.js")
}

pub async fn serve_style_css() -> Result<impl IntoResponse, AppError> {
    serve_file("style.css")
}

fn serve_page(path: &str, csp: &'static str) -> Result<Response, AppError> {
    let response = serve_file(path)?;
    Ok((
        [(X_FRAME_OPTIONS, "DENY"), (CONTENT_SECURITY_POLICY, csp)],
        response,
    )
        .into_response())
}

fn serve_file(path: &str) -> Result<Response, AppError> {
    let asset = WebAssets::get(path).ok_or(AppError::NotFound)?;
    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    Ok((
        [
            (CONTENT_TYPE, content_type.as_str()),
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        asset.data,
    )
        .into_response())
}

#[cfg(test)]
#[path = "webui_tests.rs"]
mod tests;
