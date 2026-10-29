use axum::{
    body::Bytes,
    extract::State,
    http::{
        HeaderMap,
        header,
    },
    response::IntoResponse,
};
use hmac::{
    Hmac,
    KeyInit,
    Mac,
};
use serde::Serialize;
use serde_json::Value;
use sha2::Sha256;
use tracing::info;

use crate::{
    api::shared::{
        auth::{
            bearer_token,
            token_matches,
        },
        cache::NO_STORE_CACHE_CONTROL,
        dto::ApiSuccess,
    },
    core::error::AppError,
    services::AppState,
};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize)]
pub struct WebhookSyncData {
    pub message: &'static str,
}

pub async fn handle_webhook_sync(
    headers: HeaderMap,
    State(state): State<AppState>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
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

    let payload = serde_json::from_slice::<Value>(&body).ok();
    let force = payload
        .as_ref()
        .and_then(|v| v.get("force").and_then(Value::as_bool))
        .unwrap_or(false);

    // GitHub payload 含推送者邮箱等个人信息，只记录定位一次推送所需的少数字段
    let field = |name: &str| {
        payload
            .as_ref()
            .and_then(|v| v.get(name))
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_owned()
    };
    info!(
        event = %event_type,
        force,
        action = %field("action"),
        git_ref = %field("ref"),
        after = %field("after"),
        body_len = body.len(),
        "Webhook sync triggered via API"
    );
    let state_clone = state;
    tokio::spawn(async move {
        if let Err(e) = state_clone.syncer.sync(force).await {
            tracing::error!("Webhook triggered sync failed: {e:?}");
        }
    });

    Ok((
        [(header::CACHE_CONTROL, NO_STORE_CACHE_CONTROL)],
        ApiSuccess(WebhookSyncData {
            message: "Sync triggered",
        }),
    ))
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
    if bearer_token(headers).is_some_and(|token| token_matches(token, secret)) {
        return true;
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
