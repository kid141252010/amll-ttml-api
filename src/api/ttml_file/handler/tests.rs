use super::*;

#[test]
fn resolves_supported_platforms() {
    assert!(matches!(
        resolve_platform("ncm-lyrics"),
        Ok(PlatformKind::Ncm)
    ));
    assert!(matches!(
        resolve_platform("qq-lyrics"),
        Ok(PlatformKind::Qq)
    ));
    assert!(matches!(
        resolve_platform("am-lyrics"),
        Ok(PlatformKind::Am)
    ));
    assert!(matches!(
        resolve_platform("spotify-lyrics"),
        Ok(PlatformKind::Spotify)
    ));
}

#[test]
fn rejects_unknown_platform() {
    // 注意：旧的瞎猜值（ncm/qq/apple/spotify）现在必须拒绝
    assert!(resolve_platform("ncm").is_err());
    assert!(resolve_platform("qq").is_err());
    assert!(resolve_platform("kugou").is_err());
    assert!(resolve_platform("").is_err());
}

#[test]
fn strips_ttml_suffix() {
    assert_eq!(strip_ttml_suffix("123.ttml"), Some("123"));
    assert_eq!(strip_ttml_suffix("abc123.ttml"), Some("abc123"));
}

#[test]
fn rejects_non_ttml_files() {
    assert_eq!(strip_ttml_suffix("123.xml"), None);
    assert_eq!(strip_ttml_suffix("123.ttm"), None);
    assert_eq!(strip_ttml_suffix("123.TTML"), None);
    assert_eq!(strip_ttml_suffix("123"), None);
}
