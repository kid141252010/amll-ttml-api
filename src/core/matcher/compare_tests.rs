use super::*;

#[test]
fn compare_name_perfect_match() {
    assert_eq!(
        compare_name(Some("me!"), Some("me!")),
        NameMatchType::Perfect
    );
}

#[test]
fn compare_name_dash_paren_equivalence() {
    // "Song - Remix" vs "Song (Remix)"
    assert_eq!(
        compare_name(Some("song - remix"), Some("song (remix)")),
        NameMatchType::VeryHigh
    );
}

#[test]
fn compare_name_no_match() {
    assert_eq!(
        compare_name(Some("completely different"), Some("another song")),
        NameMatchType::NoMatch
    );
}
