use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::HeaderMap,
};
use hmac::{
    Hmac,
    KeyInit,
    Mac,
};
use serde_json::{
    Value,
    json,
};
use sha2::Sha256;
use tracing::info;

use crate::{
    core::error::AppError,
    services::AppState,
};

type HmacSha256 = Hmac<Sha256>;

pub async fn handle_webhook_sync(
    headers: HeaderMap,
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    // 触发同步是非幂等操作，而 TLS 1.3 的 0-RTT 早期数据可被重放，
    // 因此在做任何签名校验之前先拒绝，要求客户端在完整握手后重试
    if is_early_data(&headers) {
        return Err(AppError::TooEarly);
    }

    let expected_secret = state.sync_secret.as_deref().ok_or_else(|| {
        AppError::InternalServerError(
            "SYNC_SECRET environment variable is not configured on the server.".to_string(),
        )
    })?;

    if !verify_webhook_auth(&headers, &body, expected_secret) {
        return Err(AppError::Unauthorized);
    }

    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    let force = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("force").and_then(Value::as_bool))
        .unwrap_or(false);

    if let Ok(payload_json) = serde_json::from_slice::<Value>(&body) {
        info!(
            event = %event_type,
            force,
            payload = %payload_json,
            "Webhook sync triggered via API"
        );
    } else if !body.is_empty() {
        let body_str = String::from_utf8_lossy(&body);
        info!(
            event = %event_type,
            force,
            body = %body_str,
            "Webhook sync triggered via API"
        );
    } else {
        info!(
            event = %event_type,
            force,
            "Webhook sync triggered via API with empty body"
        );
    }
    let state_clone = state;
    tokio::spawn(async move {
        if let Err(e) = state_clone.syncer.sync(force).await {
            tracing::error!("Webhook triggered sync failed: {e:?}");
        }
    });

    Ok(Json(json!({
        "status": 200,
        "message": "Sync triggered"
    })))
}

/// 请求是否随 TLS 1.3 的 0-RTT 早期数据到达
///
/// nginx 侧配了 `ssl_early_data on` 与 `proxy_set_header Early-Data $ssl_early_data;`。
/// 变量为空时 nginx 会省略该头，所以只有走了早期数据的请求才带 `Early-Data: 1`。
/// 读接口重放无害，不做拦截
fn is_early_data(headers: &HeaderMap) -> bool {
    headers
        .get("Early-Data")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "1")
}

fn verify_webhook_auth(headers: &HeaderMap, body: &[u8], secret: &str) -> bool {
    // Bearer Token
    if let Some(auth_header) = headers.get("Authorization").and_then(|h| h.to_str().ok()) {
        let expected_header = format!("Bearer {secret}");
        if auth_header == expected_header {
            return true;
        }
    }

    // GitHub 原生 X-Hub-Signature-256 HMAC-SHA256 签名
    if let Some(sig_header) = headers
        .get("X-Hub-Signature-256")
        .and_then(|h| h.to_str().ok())
        && let Some(hex_sig) = sig_header.strip_prefix("sha256=")
        && let Ok(expected_mac) = hex::decode(hex_sig)
        && let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes())
    {
        mac.update(body);
        if mac.verify_slice(&expected_mac).is_ok() {
            return true;
        }
    }

    false
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
