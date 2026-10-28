use super::truncate_utf8;

#[test]
fn truncate_utf8_keeps_short_strings() {
    assert_eq!(truncate_utf8("abc", 10), "abc");
    assert_eq!(truncate_utf8("abc", 3), "abc");
}

#[test]
fn truncate_utf8_never_splits_a_character() {
    // 「周」是 3 字节，上限落在字符中间时退回到前一个边界
    assert_eq!(truncate_utf8("周杰伦", 4), "周");
    assert_eq!(truncate_utf8("周杰伦", 6), "周杰");
    assert_eq!(truncate_utf8("周杰伦", 2), "");
}
