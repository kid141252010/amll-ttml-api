use super::*;
use crate::core::test_utils::make_song;

#[test]
fn lyric_source_conversion_and_serialization() {
    assert_eq!(LyricSource::from("private"), LyricSource::Private);
    assert_eq!(LyricSource::from("public"), LyricSource::Public);
    assert_eq!(LyricSource::from("bogus"), LyricSource::Public);
    assert_eq!(LyricSource::from(""), LyricSource::Public);
    assert!(LyricSource::Private.rank() < LyricSource::Public.rank());

    let serialized = serde_json::to_string(&LyricSource::Private).unwrap();
    assert_eq!(serialized, r#""private""#);
    assert_eq!(
        serde_json::from_str::<LyricSource>(&serialized).unwrap(),
        LyricSource::Private
    );
}

#[test]
fn from_entries_empty() {
    let index = LyricIndexDB::from_entries(Vec::new());
    assert!(index.entries.is_empty());
    assert!(index.id_idx.is_empty());
    assert!(index.spotify_idx.is_empty());
}

#[test]
fn from_entries_builds_platform_indices() {
    let index = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            1,
            &["Song A"],
            &["Artist X"],
            &["111"],
            &["sp1"],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            2,
            &["Song B"],
            &["Artist Y"],
            &[],
            &["sp1"],
            &[],
            &[],
        ),
    ]);

    let mut spotify_hits = index.spotify_idx.get("sp1").cloned().unwrap_or_default();
    spotify_hits.sort_unstable();
    assert_eq!(spotify_hits, vec![0, 1]);

    assert_eq!(index.ncm_idx.get("111").cloned(), Some(vec![0]));
    assert_eq!(index.entries.len(), 2);
}

#[test]
fn from_entries_id_collision_last_wins() {
    let a = make_song("a.ttml", 1, &["Song A"], &["Artist X"], &[], &[], &[], &[]);
    let mut b = make_song("b.ttml", 2, &["Song B"], &["Artist Y"], &[], &[], &[], &[]);
    b.id = a.id;
    let colliding_id = b.id;

    let index = LyricIndexDB::from_entries(vec![a, b]);
    assert_eq!(index.id_idx.get(&colliding_id), Some(&1));
}
