use super::*;
use crate::core::{
    matcher::test_support::make_entry,
    models::SearchQuery,
};

#[test]
fn score_perfect_match() {
    let query = SearchQuery {
        track_name: Some("ME!".into()),
        artist_name: Some("Taylor Swift".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME!"], &["Taylor Swift"], &["Lover"]);
    let score = score_entry(&prepared, &entry);
    assert!(score >= MatchType::VeryHigh);
}

#[test]
fn score_global_keyword_with_artist_extraction() {
    let query = SearchQuery {
        global_keyword: Some("Taylor Swift ME!".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME!"], &["Taylor Swift"], &["Lover"]);
    let score = score_entry(&prepared, &entry);
    assert!(score >= MatchType::High);
}

#[test]
fn score_no_match() {
    let query = SearchQuery {
        track_name: Some("Completely Different Song".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME!"], &["Taylor Swift"], &["Lover"]);
    let score = score_entry(&prepared, &entry);
    assert_eq!(score, MatchType::NoMatch);
}
