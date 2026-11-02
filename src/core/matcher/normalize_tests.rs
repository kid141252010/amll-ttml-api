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

#[test]
fn convert_simplified_terms_with_me_unchanged() {
    for input in ["什么", "这么", "为什么", "么"] {
        assert_eq!(convert_tw2s(input), input);
    }
}

#[test]
fn convert_traditional_terms_to_simplified() {
    for (input, expected) in [
        ("什麼", "什么"),
        ("我們", "我们"),
        ("現在是什麼時辰了", "现在是什么时辰了"),
    ] {
        assert_eq!(convert_tw2s(input), expected);
    }
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
