use super::*;
use crate::core::{
    matcher::test_support::{
        make_entry,
        make_full_entry,
    },
    models::SearchQuery,
};

#[test]
fn rough_match_global_keyword_matches_track() {
    let query = SearchQuery {
        global_keyword: Some("ME!".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME! (feat. Brendon Urie)"], &["Taylor Swift"], &["Lover"]);
    assert!(rough_match(&prepared, &entry));
}

#[test]
fn rough_match_global_keyword_matches_artist() {
    let query = SearchQuery {
        global_keyword: Some("Taylor Swift".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["Love Story"], &["Taylor Swift"], &["Fearless"]);
    assert!(rough_match(&prepared, &entry));
}

#[test]
fn rough_match_global_keyword_no_match() {
    let query = SearchQuery {
        global_keyword: Some("NonExistent Artist".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME!"], &["Taylor Swift"], &["Lover"]);
    assert!(!rough_match(&prepared, &entry));
}

#[test]
fn rough_match_track_name_contains() {
    let query = SearchQuery {
        track_name: Some("Love".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["Love Story"], &["Taylor Swift"], &["Fearless"]);
    assert!(rough_match(&prepared, &entry));
}

#[test]
fn rough_match_track_name_no_match() {
    let query = SearchQuery {
        track_name: Some("NonExistent".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME!"], &["Taylor Swift"], &["Lover"]);
    assert!(!rough_match(&prepared, &entry));
}

#[test]
fn rough_match_artist_name_and() {
    let query = SearchQuery {
        track_name: Some("ME!".into()),
        artist_name: Some("Taylor".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME!"], &["Taylor Swift"], &["Lover"]);
    assert!(rough_match(&prepared, &entry));
}

#[test]
fn rough_match_artist_name_and_fails() {
    let query = SearchQuery {
        track_name: Some("ME!".into()),
        artist_name: Some("Ed Sheeran".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_entry(&["ME!"], &["Taylor Swift"], &["Lover"]);
    assert!(!rough_match(&prepared, &entry));
}

#[test]
fn rough_match_author_id_exact() {
    let query = SearchQuery {
        author_id: Some("108002475".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_full_entry(
        &["ME!"],
        &["Taylor Swift"],
        &["Lover"],
        &["108002475"],
        &["apoint123"],
    );
    assert!(rough_match(&prepared, &entry));
}

#[test]
fn rough_match_author_id_exact_no_match() {
    let query = SearchQuery {
        author_id: Some("999999999".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_full_entry(
        &["ME!"],
        &["Taylor Swift"],
        &["Lover"],
        &["108002475"],
        &["apoint123"],
    );
    assert!(!rough_match(&prepared, &entry));
}

#[test]
fn rough_match_author_username_exact() {
    let query = SearchQuery {
        author_username: Some("apoint123".into()),
        ..Default::default()
    };
    let prepared = PreparedQuery::from_search_query(&query);
    let entry = make_full_entry(
        &["ME!"],
        &["Taylor Swift"],
        &["Lover"],
        &["108002475"],
        &["apoint123"],
    );
    assert!(rough_match(&prepared, &entry));
}
