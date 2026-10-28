/// 不区分大小写的 ASCII 字符串包含匹配
pub fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let needle_bytes = needle.as_bytes();
    let needle_len = needle_bytes.len();

    if needle_len == 0 {
        return true;
    }

    if needle_len > haystack.len() {
        return false;
    }

    haystack
        .as_bytes()
        .windows(needle_len)
        .any(|window| window.eq_ignore_ascii_case(needle_bytes))
}

/// 截断到不超过 `max_bytes` 字节，且不切断 UTF-8 字符
pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    &s[..s.floor_char_boundary(max_bytes)]
}

#[cfg(test)]
#[path = "string_tests.rs"]
mod tests;
