use super::*;

#[test]
fn test_valid_lyric_id_creation_and_get() {
    let id = LyricId::from_u64(12345).unwrap();
    assert_eq!(id.get(), 12345);
    assert_eq!(u64::from(id), 12345);
}

#[test]
fn test_max_safe_integer_boundary() {
    let max_id = LyricId::from_u64(LyricId::MAX).unwrap();
    assert_eq!(max_id.get(), 9_007_199_254_740_991);

    let over_boundary = LyricId::from_u64(LyricId::MAX + 1);
    assert!(over_boundary.is_err());
}

#[test]
fn test_from_u64_masked() {
    let masked = LyricId::from_u64_masked(u64::MAX);
    assert_eq!(masked.get(), LyricId::MAX);
}

#[test]
fn test_parse_valid_and_invalid() {
    let parsed = LyricId::parse("269710089745311").unwrap();
    assert_eq!(parsed.get(), 269_710_089_745_311);

    let invalid_str = LyricId::parse("abc");
    assert!(invalid_str.is_err());

    let overflow = LyricId::parse("9007199254740992");
    assert!(overflow.is_err());
}

#[test]
fn test_from_filename_deterministic() {
    let id1 = LyricId::from_filename("test.ttml");
    let id2 = LyricId::from_filename("test.ttml");
    assert_eq!(id1, id2);
    assert!(id1.get() <= LyricId::MAX);
}

#[test]
fn test_from_private_filename_deterministic() {
    let id1 = LyricId::from_private_filename("test.ttml");
    let id2 = LyricId::from_private_filename("test.ttml");
    let public_id = LyricId::from_filename("test.ttml");

    assert_eq!(id1, id2);
    assert_ne!(id1, public_id);
    assert!(id1.get() <= LyricId::MAX);
}
