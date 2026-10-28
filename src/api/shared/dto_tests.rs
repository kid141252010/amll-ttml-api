use compact_str::CompactString;

use super::*;
use crate::core::models::LyricSource;

#[test]
fn map_song_to_item_includes_id() {
    let filename = "1768754400682-250306205-r6IrpmBd.ttml";
    let song = SongEntry {
        id: LyricId::from_filename(filename),
        filename: CompactString::new(filename),
        timestamp: 1,
        source: LyricSource::Public,
        track_names: vec![CompactString::new("ME!")].into_boxed_slice(),
        artist_names: Box::default(),
        album_names: Box::default(),
        normalized_track_names: vec![CompactString::new("me!")].into_boxed_slice(),
        normalized_artist_names: Box::default(),
        normalized_album_names: Box::default(),
        ncm_music_ids: Box::default(),
        qq_music_ids: Box::default(),
        apple_music_ids: Box::default(),
        spotify_ids: Box::default(),
        isrcs: Box::default(),
        author_ids: Box::default(),
        author_usernames: Box::default(),
    };

    let item = map_song_to_item(&song, None, None, None);
    assert_eq!(item.id, song.id);
    assert_eq!(item.filename, filename);

    let json = serde_json::to_value(&item).unwrap();
    assert!(json.get("id").unwrap().is_number());
    assert_eq!(json.get("id").unwrap().as_u64().unwrap(), song.id.get());
}

#[test]
fn created_at_serializes_as_epoch_millis_number() {
    let filename = "1768754400682-250306205-r6IrpmBd.ttml";
    let song = SongEntry {
        id: LyricId::from_filename(filename),
        filename: CompactString::new(filename),
        timestamp: 1_768_754_400_682,
        source: LyricSource::Public,
        track_names: Box::default(),
        artist_names: Box::default(),
        album_names: Box::default(),
        normalized_track_names: Box::default(),
        normalized_artist_names: Box::default(),
        normalized_album_names: Box::default(),
        ncm_music_ids: Box::default(),
        qq_music_ids: Box::default(),
        apple_music_ids: Box::default(),
        spotify_ids: Box::default(),
        isrcs: Box::default(),
        author_ids: Box::default(),
        author_usernames: Box::default(),
    };

    let json = serde_json::to_value(map_song_to_item(&song, None, None, None)).unwrap();
    assert_eq!(json["createdAt"], 1_768_754_400_682_u64);
    assert!(json.get("timestamp").is_none());
}

#[test]
fn search_data_serialization_includes_nested_pagination() {
    let response = SuccessResponse {
        status: HttpOk,
        data: SearchData {
            items: vec![],
            pagination: PaginationInfo {
                page: Some(1),
                page_size: 20,
                total: 156,
                total_pages: Some(8),
                has_more: true,
                next_cursor: None,
            },
        },
    };

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["status"], 200);

    let data = &json["data"];
    assert!(data.get("items").unwrap().is_array());

    let pagination = &data["pagination"];
    assert_eq!(pagination["page"], 1);
    assert_eq!(pagination["pageSize"], 20);
    assert_eq!(pagination["total"], 156);
    assert_eq!(pagination["totalPages"], 8);
    assert_eq!(pagination["hasMore"], true);
}

#[test]
fn song_item_omits_absent_optional_fields() {
    let filename = "1768754400682-250306205-r6IrpmBd.ttml";
    let song = SongEntry {
        id: LyricId::from_filename(filename),
        filename: CompactString::new(filename),
        timestamp: 1,
        source: LyricSource::Public,
        track_names: vec![CompactString::new("ME!")].into_boxed_slice(),
        artist_names: Box::default(),
        album_names: Box::default(),
        normalized_track_names: Box::default(),
        normalized_artist_names: Box::default(),
        normalized_album_names: Box::default(),
        ncm_music_ids: Box::default(),
        qq_music_ids: Box::default(),
        apple_music_ids: Box::default(),
        spotify_ids: Box::default(),
        isrcs: Box::default(),
        author_ids: Box::default(),
        author_usernames: Box::default(),
    };

    let bare = map_song_to_item(&song, None, None, None);
    let json = serde_json::to_value(&bare).unwrap();
    assert!(json.get("lyrics").is_none());
    assert!(json.get("format").is_none());
    assert!(json.get("matchContext").is_none());

    let full = map_song_to_item(
        &song,
        Some("<tt/>".to_string()),
        Some("ttml".to_string()),
        Some(MatchContext {
            snippet: Some("a <mark>b</mark> c".to_string()),
        }),
    );
    let json = serde_json::to_value(&full).unwrap();
    assert_eq!(json["lyrics"], "<tt/>");
    assert_eq!(json["format"], "ttml");
    assert_eq!(json["matchContext"]["snippet"], "a <mark>b</mark> c");
}

#[test]
fn bad_request_message_is_client_facing() {
    let err = AppError::BadRequest("Missing params".into());
    let (status, error, message) = error_parts(&err);
    assert_eq!((status, error), (400, "Bad Request"));
    assert_eq!(message, "Missing params");
}

#[test]
fn not_found_messages_are_client_facing() {
    let (status, error, message) = error_parts(&AppError::NotFound);
    assert_eq!((status, error), (404, "Not Found"));
    assert_eq!(message, "The requested API route does not exist.");

    let (status, _, message) = error_parts(&AppError::LyricNotFound);
    assert_eq!(status, 404);
    assert_eq!(message, "No lyrics found for the provided query.");
}

#[test]
fn server_error_messages_are_generic() {
    let (status, _, message) = error_parts(&AppError::InternalServerError(
        "FTS Query Error: database is locked".into(),
    ));
    assert_eq!(status, 500);
    assert_eq!(message, "An internal error occurred.");

    let (status, _, message) = error_parts(&AppError::UpstreamError(
        "context: Failed to download zip: connection reset".into(),
    ));
    assert_eq!(status, 502);
    assert_eq!(message, "Upstream service unavailable.");
}

#[test]
fn error_response_serialization() {
    let (status, error, message) = error_parts(&AppError::BadRequest("test".into()));
    let payload = ErrorResponse {
        status,
        error,
        message,
    };
    let json = serde_json::to_string(&payload).unwrap();
    assert_eq!(
        json,
        r#"{"status":400,"error":"Bad Request","message":"test"}"#
    );
}
