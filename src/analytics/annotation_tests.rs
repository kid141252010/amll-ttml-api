use super::*;

fn id(n: u64) -> LyricId {
    LyricId::from_u64(n).unwrap()
}

fn track_query(track: &str, artist: &str) -> SearchQuery {
    SearchQuery {
        track_name: Some(track.to_string()),
        artist_name: Some(artist.to_string()),
        ..SearchQuery::default()
    }
}

#[test]
fn disabled_annotator_drops_everything() {
    let annotator = Annotator::default();
    annotator.listing(3, Some((id(1), None)), None);
    assert_eq!(annotator.take(), None);
}

#[test]
fn lookup_hit_records_id_and_kind() {
    let annotator = Annotator::enabled();
    let result: Result<LyricId, AppError> = Ok(id(42));
    annotator.lookup(&result, |&hit| hit, MatchKind::PlatformId, None);

    assert_eq!(
        annotator.take(),
        Some(Annotation {
            hit_count: Some(1),
            hit_id: Some(id(42)),
            match_kind: Some(MatchKind::PlatformId),
            norm_query: None,
        })
    );
}

#[test]
fn lookup_miss_records_normalized_query() {
    let annotator = Annotator::enabled();
    let result: Result<LyricId, AppError> = Err(AppError::LyricNotFound);
    let query = track_query("晴天", "周杰倫");
    annotator.lookup(&result, |&hit| hit, MatchKind::Fuzzy, Some(&query));

    let annotation = annotator.take().unwrap();
    assert_eq!(annotation.hit_count, Some(0));
    assert_eq!(annotation.hit_id, None);
    assert_eq!(annotation.match_kind, Some(MatchKind::Fuzzy));
    // 繁体歌手名经 OpenCC 转成简体，与检索口径一致
    assert_eq!(
        annotation.norm_query.as_deref(),
        Some(r#"{"track":"晴天","artist":"周杰伦"}"#)
    );
}

#[test]
fn lookup_ignores_server_errors() {
    let annotator = Annotator::enabled();
    let result: Result<LyricId, AppError> = Err(AppError::UpstreamError("down".into()));
    annotator.lookup(&result, |&hit| hit, MatchKind::Id, None);
    assert_eq!(annotator.take(), None);
}

#[test]
fn listing_only_normalizes_on_zero_hits() {
    let query = track_query("Song", "Artist");

    let annotator = Annotator::enabled();
    annotator.listing(5, Some((id(7), Some(MatchKind::Fts))), Some(&query));
    let annotation = annotator.take().unwrap();
    assert_eq!(annotation.hit_count, Some(5));
    assert_eq!(annotation.hit_id, Some(id(7)));
    assert_eq!(annotation.match_kind, Some(MatchKind::Fts));
    assert_eq!(annotation.norm_query, None);

    let annotator = Annotator::enabled();
    annotator.listing(0, None, Some(&query));
    let annotation = annotator.take().unwrap();
    assert_eq!(annotation.hit_count, Some(0));
    assert_eq!(annotation.match_kind, None);
    assert_eq!(
        annotation.norm_query.as_deref(),
        Some(r#"{"track":"song","artist":"artist"}"#)
    );
}

#[test]
fn normalized_query_skips_non_text_conditions() {
    let lyric_only = SearchQuery {
        lyric_text: Some("hello".to_string()),
        author_id: Some("123".to_string()),
        ..SearchQuery::default()
    };
    assert_eq!(normalized_query(&lyric_only), None);
}

#[test]
fn normalized_query_truncates_long_fields() {
    let query = SearchQuery {
        global_keyword: Some("a".repeat(1000)),
        ..SearchQuery::default()
    };
    let json = normalized_query(&query).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["q"].as_str().unwrap().len(), MAX_NORM_FIELD_BYTES);
}

#[test]
fn match_kind_follows_id_query_priority() {
    let mut query = IdQuery {
        ncm_music_ids: vec!["1".to_string()],
        ..IdQuery::default()
    };
    assert_eq!(MatchKind::for_id_query(&query), MatchKind::PlatformId);

    query.filename = Some("a.ttml".to_string());
    assert_eq!(MatchKind::for_id_query(&query), MatchKind::Filename);

    query.id = Some(id(1));
    assert_eq!(MatchKind::for_id_query(&query), MatchKind::Id);
}
