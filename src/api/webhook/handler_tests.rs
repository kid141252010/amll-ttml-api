use axum::{
    body::Bytes,
    extract::State,
    http::HeaderMap,
};
use hmac::{
    Hmac,
    KeyInit,
    Mac,
};
use sha2::Sha256;

use super::handle_webhook_sync;
use crate::{
    core::error::AppError,
    init_db,
    services::AppState,
};

fn compute_github_signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let result = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(result))
}

#[tokio::test]
async fn test_webhook_sync_auth_bearer() {
    let db_conn = init_db("sqlite::memory:").await.unwrap();

    let state_no_secret = AppState::new_with_secret(db_conn.clone(), None);
    let headers_empty = HeaderMap::new();
    let res = handle_webhook_sync(headers_empty, State(state_no_secret), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::InternalServerError(_))));

    let state = AppState::new_with_secret(db_conn, Some("test_secret_123"));

    let mut headers = HeaderMap::new();
    let res = handle_webhook_sync(headers.clone(), State(state.clone()), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::Unauthorized)));

    headers.insert("Authorization", "test_secret_123".parse().unwrap());
    let res = handle_webhook_sync(headers.clone(), State(state.clone()), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::Unauthorized)));

    headers.insert("Authorization", "Bearer test_secret_123".parse().unwrap());
    let res = handle_webhook_sync(headers, State(state), Bytes::new()).await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_webhook_sync_auth_github_hmac() {
    let db_conn = init_db("sqlite::memory:").await.unwrap();
    let secret = "github_webhook_secret_key";
    let state = AppState::new_with_secret(db_conn, Some(secret));

    let payload =
        Bytes::from_static(b"{\"action\": \"closed\", \"pull_request\": {\"merged\": true}}");
    let valid_sig = compute_github_signature(secret, &payload);

    let mut headers = HeaderMap::new();
    headers.insert("X-Hub-Signature-256", valid_sig.parse().unwrap());
    let res = handle_webhook_sync(headers, State(state.clone()), payload.clone()).await;
    assert!(res.is_ok());

    let tampered_payload = Bytes::from_static(b"{\"action\": \"opened\"}");
    let mut headers = HeaderMap::new();
    headers.insert("X-Hub-Signature-256", valid_sig.parse().unwrap());
    let res = handle_webhook_sync(headers, State(state.clone()), tampered_payload).await;
    assert!(matches!(res, Err(AppError::Unauthorized)));

    let mut headers = HeaderMap::new();
    headers.insert("X-Hub-Signature-256", "sha1=invalid".parse().unwrap());
    let res = handle_webhook_sync(headers, State(state), payload).await;
    assert!(matches!(res, Err(AppError::Unauthorized)));
}

#[tokio::test]
async fn test_webhook_sync_force_payload() {
    let db_conn = init_db("sqlite::memory:").await.unwrap();
    let state = AppState::new_with_secret(db_conn, Some("test_secret"));

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_secret".parse().unwrap());

    let payload = Bytes::from_static(b"{\"force\": true}");
    let res = handle_webhook_sync(headers, State(state), payload).await;
    assert!(res.is_ok());
}

/// 0-RTT 早期数据可被重放，即使签名合法也必须拒绝
#[tokio::test]
async fn test_webhook_sync_rejects_tls_early_data() {
    let db_conn = init_db("sqlite::memory:").await.unwrap();
    let state = AppState::new_with_secret(db_conn, Some("test_secret"));

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_secret".parse().unwrap());
    headers.insert("Early-Data", "1".parse().unwrap());
    let res = handle_webhook_sync(headers, State(state.clone()), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::TooEarly)));

    // 早期数据检查在签名校验之前，未授权请求同样先得到 425
    let mut headers = HeaderMap::new();
    headers.insert("Early-Data", "1".parse().unwrap());
    let res = handle_webhook_sync(headers, State(state.clone()), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::TooEarly)));

    // 完整握手的请求不带该头，nginx 在变量为空时会省略，正常放行
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_secret".parse().unwrap());
    let res = handle_webhook_sync(headers, State(state.clone()), Bytes::new()).await;
    assert!(res.is_ok());

    // 只有值为 1 才算早期数据
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_secret".parse().unwrap());
    headers.insert("Early-Data", "0".parse().unwrap());
    let res = handle_webhook_sync(headers, State(state), Bytes::new()).await;
    assert!(res.is_ok());
}
