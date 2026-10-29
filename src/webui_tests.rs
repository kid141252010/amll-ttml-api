use axum::{
    body::Body,
    http::{
        HeaderMap,
        Request,
        StatusCode,
    },
};
use tower::ServiceExt;

use super::*;
use crate::{
    core::db::setup::init_db,
    create_app,
    services::AppState,
};

async fn get_headers(uri: &str) -> (StatusCode, HeaderMap) {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let response = create_app(AppState::new_with_secret(conn, None))
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    (response.status(), response.headers().clone())
}

#[tokio::test]
async fn pages_carry_security_headers() {
    for (uri, csp) in [("/", INDEX_CSP), ("/admin", ADMIN_CSP)] {
        let (status, headers) = get_headers(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(
            headers[CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html"),
            "{uri}"
        );
        assert_eq!(headers[X_FRAME_OPTIONS], "DENY", "{uri}");
        assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff", "{uri}");
        assert_eq!(headers[CONTENT_SECURITY_POLICY], csp, "{uri}");
    }
}

#[test]
fn page_csp_forbids_framing_and_only_admin_allows_inline_script() {
    for csp in [INDEX_CSP, ADMIN_CSP] {
        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }
    assert!(INDEX_CSP.contains("script-src 'self';"));
    assert!(!INDEX_CSP.contains("unsafe-inline"));
    assert!(ADMIN_CSP.contains("script-src 'self' 'unsafe-inline'"));
}

#[tokio::test]
async fn static_assets_carry_nosniff() {
    for uri in ["/app.js", "/style.css"] {
        let (status, headers) = get_headers(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff", "{uri}");
        assert!(headers.get(CONTENT_SECURITY_POLICY).is_none(), "{uri}");
    }
}
