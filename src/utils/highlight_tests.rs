use super::*;

#[test]
fn test_highlight_keyword_basic_casing_preserved() {
    let text = "In the END, it doesn't matter";
    let kw = "end";
    let result = highlight_keyword(text, kw);
    assert_eq!(result, "In the <mark>END</mark>, it doesn't matter");
}

#[test]
fn test_highlight_keyword_multiple_matches() {
    let text = "hello world, HELLO universe";
    let kw = "hello";
    let result = highlight_keyword(text, kw);
    assert_eq!(
        result,
        "<mark>hello</mark> world, <mark>HELLO</mark> universe"
    );
}

#[test]
fn test_highlight_keyword_chinese() {
    let text = "为你弹奏肖邦的夜曲，纪念我死去的爱情";
    let kw = "肖邦";
    let result = highlight_keyword(text, kw);
    assert_eq!(result, "为你弹奏<mark>肖邦</mark>的夜曲，纪念我死去的爱情");
}

#[test]
fn test_highlight_keyword_no_match() {
    let text = "Hello World";
    let kw = "abc";
    let result = highlight_keyword(text, kw);
    assert_eq!(result, "Hello World");
}

#[test]
fn test_highlight_keyword_empty_inputs() {
    assert_eq!(highlight_keyword("", "abc"), "");
    assert_eq!(highlight_keyword("Hello", ""), "Hello");
}

#[test]
fn test_extract_lyric_context_middle_line() {
    let lyrics = "第一句歌词\n第二句歌词 肖邦\n第三句歌词\n第四句歌词";
    let kw = "肖邦";
    let context = extract_lyric_context(lyrics, kw).unwrap();
    let expected = "第一句歌词\n第二句歌词 <mark>肖邦</mark>\n第三句歌词";
    assert_eq!(context, expected);
}

#[test]
fn test_extract_lyric_context_first_line_boundary() {
    let lyrics = "第一句歌词 肖邦\n第二句歌词\n第三句歌词";
    let kw = "肖邦";
    let context = extract_lyric_context(lyrics, kw).unwrap();
    let expected = "第一句歌词 <mark>肖邦</mark>\n第二句歌词";
    assert_eq!(context, expected);
}

#[test]
fn test_extract_lyric_context_last_line_boundary() {
    let lyrics = "第一句歌词\n第二句歌词\n第三句歌词 肖邦";
    let kw = "肖邦";
    let context = extract_lyric_context(lyrics, kw).unwrap();
    let expected = "第二句歌词\n第三句歌词 <mark>肖邦</mark>";
    assert_eq!(context, expected);
}

#[test]
fn test_extract_lyric_context_single_line() {
    let lyrics = "唯一的歌词 肖邦";
    let kw = "肖邦";
    let context = extract_lyric_context(lyrics, kw).unwrap();
    assert_eq!(context, "唯一的歌词 <mark>肖邦</mark>");
}

#[test]
fn test_extract_lyric_context_no_match() {
    let lyrics = "第一句歌词\n第二句歌词";
    let kw = "肖邦";
    assert!(extract_lyric_context(lyrics, kw).is_none());
}

#[test]
fn test_extract_lyric_context_empty_inputs() {
    assert!(extract_lyric_context("", "肖邦").is_none());
    assert!(extract_lyric_context("第一句歌词", "").is_none());
}
