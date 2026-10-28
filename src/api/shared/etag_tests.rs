use axum::http::{
    HeaderMap,
    HeaderValue,
    header::IF_NONE_MATCH,
};

use super::{
    any_if_none_match_hit,
    if_none_match_hit,
    strip_weak,
};

const TAG: &str = "W/\"0123456789abcdef\"";

fn appended(lines: &[&'static str]) -> Vec<HeaderValue> {
    let mut headers = HeaderMap::new();
    for line in lines {
        headers.append(IF_NONE_MATCH, HeaderValue::from_static(line));
    }
    headers.get_all(IF_NONE_MATCH).iter().cloned().collect()
}

#[test]
fn strip_weak_removes_prefix_only_when_present() {
    assert_eq!(strip_weak("W/\"abc\""), "\"abc\"");
    assert_eq!(strip_weak("\"abc\""), "\"abc\"");
}

#[test]
fn weak_and_strong_forms_compare_equal() {
    let strong = HeaderValue::from_static("\"0123456789abcdef\"");
    assert!(if_none_match_hit(&strong, TAG));
}

#[test]
fn identical_weak_form_hits() {
    let same = HeaderValue::from_static("W/\"0123456789abcdef\"");
    assert!(if_none_match_hit(&same, TAG));
}

#[test]
fn wildcard_hits() {
    let wildcard = HeaderValue::from_static("*");
    assert!(if_none_match_hit(&wildcard, TAG));
}

#[test]
fn tag_list_hits_when_any_member_matches() {
    let list = HeaderValue::from_static("W/\"deadbeef\", W/\"0123456789abcdef\"");
    assert!(if_none_match_hit(&list, TAG));
}

#[test]
fn unrelated_tag_misses() {
    let other = HeaderValue::from_static("W/\"deadbeef\"");
    assert!(!if_none_match_hit(&other, TAG));
}

#[test]
fn non_ascii_header_misses_without_panicking() {
    let invalid = HeaderValue::from_bytes(&[0xff, 0xfe]).expect("opaque header value");
    assert!(!if_none_match_hit(&invalid, TAG));
}

#[test]
fn absent_header_misses() {
    assert!(!any_if_none_match_hit(&appended(&[]), TAG));
}

#[test]
fn repeated_header_lines_hit_on_any_line() {
    let lines = appended(&["W/\"deadbeef\"", "W/\"0123456789abcdef\""]);
    assert_eq!(lines.len(), 2);
    assert!(any_if_none_match_hit(&lines, TAG));
}

#[test]
fn repeated_header_lines_miss_when_none_match() {
    let lines = appended(&["W/\"deadbeef\"", "W/\"cafebabe\""]);
    assert!(!any_if_none_match_hit(&lines, TAG));
}

#[test]
fn wildcard_on_a_later_line_hits() {
    assert!(any_if_none_match_hit(
        &appended(&["W/\"deadbeef\"", "*"]),
        TAG
    ));
}
