use super::*;

// --- convert_tw2s tests ---

#[test]
fn convert_traditional_to_simplified() {
    // 繁体 "愛" -> 简体 "爱"
    let result = convert_tw2s("愛");
    assert_eq!(result, "爱");
}

#[test]
fn convert_simplified_unchanged() {
    let result = convert_tw2s("爱");
    assert_eq!(result, "爱");
}

// --- normalize_name_for_comparison tests ---

#[test]
fn normalize_curly_quotes() {
    let result = normalize_name_for_comparison("it\u{2019}s");
    assert_eq!(result, "it's");
}

#[test]
fn normalize_chinese_comma() {
    let result = normalize_name_for_comparison("A，B");
    assert_eq!(result, "a,b");
}

#[test]
fn normalize_brackets() {
    let result = normalize_name_for_comparison("Song [Deluxe]");
    assert_eq!(result, "song (deluxe)");
}

#[test]
fn normalize_multiple_spaces() {
    assert_eq!(normalize_name_for_comparison("A   B"), "a b");
    assert_eq!(normalize_name_for_comparison("A    B"), "a b");
    assert_eq!(normalize_name_for_comparison("  A  B  "), "a b");
}
