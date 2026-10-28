use axum::http::HeaderValue;
use hmac::KeyInit;

use super::*;

fn hasher(key: &[u8]) -> IpHasher {
    IpHasher::new_from_slice(key).unwrap()
}

fn headers_with_ip(ip: &'static str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(REAL_IP_HEADER, HeaderValue::from_static(ip));
    headers
}

#[test]
fn split_route_strips_either_prefix() {
    assert_eq!(
        split_route("/v1/lrclib/get/42", Some("/v1/lrclib/get/{id}")),
        (Some("/v1"), Some("/lrclib/get/{id}".to_string()))
    );
    assert_eq!(
        split_route("/api/v1/lyrics/get", Some("/api/v1/lyrics/get")),
        (Some("/api/v1"), Some("/lyrics/get".to_string()))
    );
}

#[test]
fn split_route_keeps_prefix_for_unmatched_paths() {
    assert_eq!(split_route("/v1/nope", None), (Some("/v1"), None));
    assert_eq!(split_route("/wp-login.php", None), (None, None));
    // 段边界之外的相似前缀不算
    assert_eq!(split_route("/v1x/lyrics", None), (None, None));
}

#[test]
fn split_route_distinguishes_ttml_links_from_api_paths() {
    assert_eq!(
        split_route("/v1/does-not-exist", Some("/{platform}/{file}")),
        (Some("/v1"), None)
    );
    assert_eq!(
        split_route("/api/v1/does-not-exist", Some("/{platform}/{file}")),
        (Some("/api/v1"), None)
    );
    assert_eq!(
        split_route("/ncm-lyrics/123.ttml", Some("/{platform}/{file}")),
        (None, Some("/{platform}/{file}".to_string()))
    );
    assert_eq!(
        split_route("/v1/lrclib/get/42", Some("/v1/lrclib/get/{id}")),
        (Some("/v1"), Some("/lrclib/get/{id}".to_string()))
    );
}

#[test]
fn params_json_decodes_and_collects_repeated_keys() {
    let json = params_json("musicName=%E6%99%B4%E5%A4%A9&ncmMusicId=1&ncmMusicId=2&x=").unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["musicName"], "晴天");
    assert_eq!(value["ncmMusicId"], serde_json::json!(["1", "2"]));
    assert_eq!(value["x"], "");
}

#[test]
fn params_json_is_bounded() {
    let long_value = "v".repeat(1000);
    let many: Vec<String> = (0..40).map(|i| format!("k{i}={long_value}")).collect();
    let json = params_json(&many.join("&")).unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();

    let object = value.as_object().unwrap();
    assert_eq!(object.len(), MAX_PARAMS);
    assert!(
        object
            .values()
            .all(|v| v.as_str().unwrap().len() == MAX_PARAM_VALUE_BYTES)
    );
}

#[test]
fn params_json_replaces_nul_and_skips_empty_query() {
    let json = params_json("q=a%00b").unwrap();
    assert_eq!(json, r#"{"q":"a b"}"#);
    assert_eq!(params_json(""), None);
}

#[test]
fn referer_keeps_only_origin() {
    assert_eq!(
        referer_origin("https://example.com:8443/player?song=1#t=3").as_deref(),
        Some("https://example.com:8443")
    );
    assert_eq!(
        referer_origin("https://example.com/").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(referer_origin("not a url"), None);
}

#[test]
fn client_id_is_stable_per_key_and_ip() {
    let key_a = hasher(b"key-a");
    let key_b = hasher(b"key-b");

    let first = client_id(&headers_with_ip("203.0.113.7"), &key_a);
    assert!(first.is_some());
    assert_eq!(first, client_id(&headers_with_ip(" 203.0.113.7 "), &key_a));
    assert_ne!(first, client_id(&headers_with_ip("203.0.113.8"), &key_a));
    assert_ne!(first, client_id(&headers_with_ip("203.0.113.7"), &key_b));
}

#[test]
fn client_id_requires_a_valid_real_ip() {
    let key = hasher(b"key");
    assert_eq!(client_id(&HeaderMap::new(), &key), None);
    assert_eq!(client_id(&headers_with_ip("unknown"), &key), None);

    // 只认 X-Real-IP，可伪造的 X-Forwarded-For 不参与
    let mut forwarded = HeaderMap::new();
    forwarded.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));
    assert_eq!(client_id(&forwarded, &key), None);
}

#[test]
fn header_text_trims_and_truncates() {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("  AMLL Player/1.0  "));
    assert_eq!(
        header_text(&headers, USER_AGENT.as_str(), 100).as_deref(),
        Some("AMLL Player/1.0")
    );
    assert_eq!(
        header_text(&headers, USER_AGENT.as_str(), 4).as_deref(),
        Some("AMLL")
    );
    assert_eq!(header_text(&headers, ORIGIN.as_str(), 100), None);
}
