use std::sync::{
    Arc,
    Mutex,
};

use axum::{
    body::{
        Bytes,
        to_bytes,
    },
    extract::State,
    http::{
        HeaderMap,
        header::CACHE_CONTROL,
    },
    response::IntoResponse,
};
use hmac::{
    Hmac,
    KeyInit,
    Mac,
};
use sha2::Sha256;

use super::handle_webhook_sync;
use crate::{
    api::shared::cache::NO_STORE_CACHE_CONTROL,
    core::error::AppError,
    init_db,
    services::{
        AppState,
        lyric_syncer::LyricSyncer,
        sync_service::{
            SyncResult,
            SyncStatus,
        },
    },
};

/// 同步步骤换成不出网的空实现，handler 里 `tokio::spawn` 的同步不会真去拉 GitHub
///
/// 返回的列表按调用顺序记下每次收到的 `force`
async fn isolated_state(secret: Option<&str>) -> (AppState, Arc<Mutex<Vec<bool>>>) {
    let db_conn = init_db("sqlite::memory:").await.unwrap();
    let mut state = AppState::new_with_secret(db_conn, secret);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    state.syncer = LyricSyncer::with_runner(state.store.clone(), move |force| {
        recorded.lock().unwrap().push(force);
        Box::pin(async {
            Ok(SyncResult {
                status: SyncStatus::Skipped,
                count: None,
                error: None,
                strategy: None,
            })
        })
    });
    (state, calls)
}

/// 等 handler 里 spawn 出去的同步跑完
async fn wait_for_calls(calls: &Mutex<Vec<bool>>, n: usize) -> Vec<bool> {
    for _ in 0..200 {
        let seen = calls.lock().unwrap().clone();
        if seen.len() >= n {
            return seen;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("sync was not triggered {n} time(s)");
}

fn compute_github_signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let result = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(result))
}

#[tokio::test]
async fn test_webhook_sync_auth_bearer() {
    let (state_no_secret, _) = isolated_state(None).await;
    let headers_empty = HeaderMap::new();
    let res = handle_webhook_sync(headers_empty, State(state_no_secret), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::InternalServerError(_))));

    let (state, calls) = isolated_state(Some("test_secret_123")).await;

    let mut headers = HeaderMap::new();
    let res = handle_webhook_sync(headers.clone(), State(state.clone()), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::Unauthorized)));

    headers.insert("Authorization", "test_secret_123".parse().unwrap());
    let res = handle_webhook_sync(headers.clone(), State(state.clone()), Bytes::new()).await;
    assert!(matches!(res, Err(AppError::Unauthorized)));

    headers.insert("Authorization", "Bearer test_secret_123".parse().unwrap());
    let res = handle_webhook_sync(headers, State(state), Bytes::new()).await;
    assert!(res.is_ok());
    assert_eq!(wait_for_calls(&calls, 1).await, vec![false]);
}

#[tokio::test]
async fn test_webhook_sync_auth_github_hmac() {
    let secret = "github_webhook_secret_key";
    let (state, calls) = isolated_state(Some(secret)).await;

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

    // 只有签名合法的那一次触发了同步
    assert_eq!(wait_for_calls(&calls, 1).await, vec![false]);
}

#[tokio::test]
async fn test_webhook_sync_force_payload() {
    let (state, calls) = isolated_state(Some("test_secret")).await;

    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_secret".parse().unwrap());

    let payload = Bytes::from_static(b"{\"force\": true}");
    let res = handle_webhook_sync(headers, State(state), payload).await;
    assert!(res.is_ok());
    assert_eq!(wait_for_calls(&calls, 1).await, vec![true]);
}

/// 成功响应走原生信封 `{status, data}`，且不进任何缓存
#[tokio::test]
async fn test_webhook_sync_success_shape_and_no_store() {
    let (state, _) = isolated_state(Some("test_secret")).await;
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer test_secret".parse().unwrap());

    let response = handle_webhook_sync(headers, State(state), Bytes::new())
        .await
        .expect("authorized")
        .into_response();

    assert_eq!(response.headers()[CACHE_CONTROL], NO_STORE_CACHE_CONTROL);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"status": 200, "data": {"message": "Sync triggered"}})
    );
}

/// 0-RTT 早期数据可被重放，即使签名合法也必须拒绝
#[tokio::test]
async fn test_webhook_sync_rejects_tls_early_data() {
    let (state, _) = isolated_state(Some("test_secret")).await;

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

#[derive(Clone, Default)]
struct CapturedLogs(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLogs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// GitHub push payload 含推送者邮箱，只允许 event / ref / after 这类定位字段进日志
#[tokio::test]
async fn test_webhook_sync_log_omits_payload_details() {
    let db_conn = init_db("sqlite::memory:").await.unwrap();
    let secret = "github_webhook_secret_key";
    let state = AppState::new_with_secret(db_conn, Some(secret));

    let payload = Bytes::from_static(
        br#"{"ref":"refs/heads/main","after":"abc123","pusher":{"name":"someone","email":"someone@example.com"}}"#,
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Hub-Signature-256",
        compute_github_signature(secret, &payload).parse().unwrap(),
    );
    headers.insert("X-GitHub-Event", "push".parse().unwrap());

    let logs = CapturedLogs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    let res = handle_webhook_sync(headers, State(state), payload).await;
    drop(guard);
    assert!(res.is_ok());

    let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(output.contains("Webhook sync triggered"), "{output}");
    assert!(output.contains("event=push"), "{output}");
    assert!(output.contains("refs/heads/main"), "{output}");
    assert!(output.contains("abc123"), "{output}");
    assert!(!output.contains("someone@example.com"), "{output}");
    assert!(!output.contains("pusher"), "{output}");
}
