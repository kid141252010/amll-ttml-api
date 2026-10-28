use super::*;

#[test]
fn test_format_lrc_timestamp() {
    assert_eq!(format_lrc_timestamp(0), "[00:00.00]");
    assert_eq!(format_lrc_timestamp(1500), "[00:01.50]");
    assert_eq!(format_lrc_timestamp(65123), "[01:05.12]");
}

#[test]
fn test_parse_and_format_ttml_complete() {
    let sample = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
        <body>
            <div>
                <p begin="00:01.500" end="00:04.000">First line<span ttm:role="x-bg" begin="00:01.500" end="00:03.500">bg vocal</span></p>
                <p begin="00:05.123" end="00:08.456">Second line</p>
            </div>
        </body>
        </tt>"#;

    let result = parse_and_format_ttml(sample);

    assert_eq!(result.duration, 8.456);
    assert_eq!(
        result.plain_lyrics,
        Some("First line\n(bg vocal)\nSecond line".to_string())
    );
    assert_eq!(
        result.synced_lyrics,
        Some("[00:01.50] First line\n[00:01.50] (bg vocal)\n[00:05.12] Second line".to_string())
    );
}
