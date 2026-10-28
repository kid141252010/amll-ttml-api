use super::*;

#[test]
fn bits_are_distinct() {
    let mut all = IdKindSet::empty();
    for kind in IdKind::ALL {
        assert!(!all.contains(kind), "{kind:?} 的位与之前的变体重叠");
        all.insert(kind);
    }
    assert_eq!(all, IdKindSet(0b0001_1111));
}

#[test]
fn insert_is_idempotent() {
    let mut set = IdKindSet::empty();
    set.insert(IdKind::Isrc);
    let once = set;
    set.insert(IdKind::Isrc);
    assert_eq!(set, once);
}

#[test]
fn empty_constraint_never_filters() {
    let empty = IdKindSet::empty();
    for present in [empty, {
        let mut set = IdKindSet::empty();
        set.insert(IdKind::SpotifyId);
        set
    }] {
        assert!(present.contains_all(empty));
        assert!(present.contains_none(empty));
    }
}

#[test]
fn contains_all_requires_every_member() {
    let mut required = IdKindSet::empty();
    required.insert(IdKind::NcmMusicId);
    required.insert(IdKind::Isrc);

    let mut partial = IdKindSet::empty();
    partial.insert(IdKind::NcmMusicId);
    assert!(!partial.contains_all(required));

    partial.insert(IdKind::Isrc);
    assert!(partial.contains_all(required));

    // 多出来的标识不影响判定
    partial.insert(IdKind::QqMusicId);
    assert!(partial.contains_all(required));
}

#[test]
fn contains_none_rejects_any_member() {
    let mut forbidden = IdKindSet::empty();
    forbidden.insert(IdKind::QqMusicId);
    forbidden.insert(IdKind::AppleMusicId);

    let mut present = IdKindSet::empty();
    present.insert(IdKind::NcmMusicId);
    assert!(present.contains_none(forbidden));

    present.insert(IdKind::AppleMusicId);
    assert!(!present.contains_none(forbidden));
}

#[test]
fn intersection_detects_overlap() {
    let mut has = IdKindSet::empty();
    has.insert(IdKind::NcmMusicId);
    has.insert(IdKind::Isrc);

    let mut disjoint = IdKindSet::empty();
    disjoint.insert(IdKind::QqMusicId);
    assert!(has.intersection(disjoint).is_empty());

    let mut overlapping = IdKindSet::empty();
    overlapping.insert(IdKind::QqMusicId);
    overlapping.insert(IdKind::Isrc);
    let overlap = has.intersection(overlapping);
    assert!(!overlap.is_empty());
    assert!(overlap.contains(IdKind::Isrc));
    assert!(!overlap.contains(IdKind::QqMusicId));
}

#[test]
fn default_filter_is_empty() {
    assert!(ListFilter::default().is_empty());

    let mut filter = ListFilter::default();
    filter.missing.insert(IdKind::Isrc);
    assert!(!filter.is_empty());

    let author_only = ListFilter {
        author_id: Some("108002475".into()),
        ..ListFilter::default()
    };
    assert!(!author_only.is_empty());

    let since_only = ListFilter {
        since: Some(1_700_000_000_000),
        ..ListFilter::default()
    };
    assert!(!since_only.is_empty());

    let until_only = ListFilter {
        until: Some(1_700_000_000_000),
        ..ListFilter::default()
    };
    assert!(!until_only.is_empty());
}

#[test]
fn cursor_parse_valid() {
    let cursor = Cursor::parse("1768754400682_699269132670751").unwrap();
    assert_eq!(cursor.timestamp, 1_768_754_400_682);
    assert_eq!(cursor.id.get(), 699_269_132_670_751);
    assert_eq!(cursor.to_compact_string(), "1768754400682_699269132670751");
}

#[test]
fn cursor_parse_invalid_format() {
    assert!(Cursor::parse("").is_err());
    assert!(Cursor::parse("1768754400682").is_err());
    assert!(Cursor::parse("1768754400682_").is_err());
    assert!(Cursor::parse("_699269132670751").is_err());
    assert!(Cursor::parse("abc_def").is_err());
    assert!(Cursor::parse("1768754400682_abc").is_err());
    assert!(Cursor::parse("-10_100").is_err());
}
