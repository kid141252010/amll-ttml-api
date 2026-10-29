//! 内部端点共用的 Bearer 鉴权与 `no-store` 响应修饰
//!
//! admin（统计下载、私库上传）与 webhook 三处共用同一套 token 比较，
//! 避免各自维护一份、其中某处退化成普通字符串比较

use axum::{
    http::{
        HeaderMap,
        header::{
            AUTHORIZATION,
            CACHE_CONTROL,
        },
    },
    response::Response,
};
use sha2::{
    Digest,
    Sha256,
};
use subtle::ConstantTimeEq;

use crate::{
    api::shared::cache::NO_STORE_CACHE_CONTROL,
    core::error::AppError,
};

/// 常量时间比较。先各自取哈希，长度差异也不会从耗时上泄露
pub fn token_matches(provided: &str, expected: &str) -> bool {
    let provided = Sha256::digest(provided.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());
    provided.as_slice().ct_eq(expected.as_slice()).into()
}

/// 取 `Authorization: Bearer <token>` 里的 token，缺失或格式不对返回 `None`
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

/// 未配置密钥是服务端配置错误（500，`env_name` 进错误消息）；Bearer 缺失或不匹配是 401
pub fn authorize_bearer(
    headers: &HeaderMap,
    secret: Option<&str>,
    env_name: &str,
) -> Result<(), AppError> {
    let secret = secret.ok_or_else(|| {
        AppError::InternalServerError(format!(
            "{env_name} environment variable is not configured on the server."
        ))
    })?;

    let provided = bearer_token(headers).ok_or(AppError::Unauthorized)?;

    if token_matches(provided, secret) {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
}

/// 带 token 的内部端点所有响应（含错误）一律 `no-store`
pub fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, NO_STORE_CACHE_CONTROL);
    response
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
