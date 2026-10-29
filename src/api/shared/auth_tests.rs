use axum::{
    http::{
        HeaderMap,
        header::{
            AUTHORIZATION,
            CACHE_CONTROL,
        },
    },
    response::IntoResponse,
};

use super::*;

fn headers_with_auth(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, value.parse().expect("valid header"));
    headers
}

#[test]
fn token_matches_accepts_identical_tokens() {
    assert!(token_matches("secret", "secret"));
    assert!(token_matches("", ""));
    assert!(token_matches("中文密钥", "中文密钥"));
}

#[test]
fn token_matches_rejects_different_tokens() {
    assert!(!token_matches("SECRET", "secret"));
    assert!(!token_matches("secreT", "secret"));
    assert!(!token_matches("Bearer secret", "secret"));
}

#[test]
fn token_matches_rejects_length_differences() {
    assert!(!token_matches("secret ", "secret"));
    assert!(!token_matches("secre", "secret"));
    assert!(!token_matches("", "secret"));
    assert!(!token_matches("secret", ""));
    assert!(!token_matches(&"secret".repeat(100), "secret"));
}

#[test]
fn bearer_token_requires_bearer_scheme() {
    assert_eq!(bearer_token(&headers_with_auth("Bearer abc")), Some("abc"));
    assert_eq!(bearer_token(&headers_with_auth("abc")), None);
    assert_eq!(bearer_token(&headers_with_auth("Basic abc")), None);
    assert_eq!(bearer_token(&HeaderMap::new()), None);
}

#[test]
fn authorize_bearer_maps_errors() {
    let ok = headers_with_auth("Bearer s3cret");
    assert!(authorize_bearer(&ok, Some("s3cret"), "TEST_SECRET").is_ok());

    for headers in [
        headers_with_auth("Bearer wrong"),
        headers_with_auth("s3cret"),
        HeaderMap::new(),
    ] {
        assert!(matches!(
            authorize_bearer(&headers, Some("s3cret"), "TEST_SECRET"),
            Err(AppError::Unauthorized)
        ));
    }

    assert!(matches!(
        authorize_bearer(&ok, None, "TEST_SECRET"),
        Err(AppError::InternalServerError(message)) if message.contains("TEST_SECRET")
    ));
}

#[test]
fn no_store_overrides_cache_control() {
    let response = no_store(AppError::NotFound.into_response());
    assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
}
