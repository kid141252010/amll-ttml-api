use sea_orm::{
    EntityTrait,
    Set,
};
use serde_json::{
    Value,
    json,
};

use super::*;
use crate::core::db::setup::init_db;

fn seed_row(
    filename: &str,
    lyric_text: &str,
    bg_vocal_text: &str,
    raw_ttml: &str,
) -> entity::ActiveModel {
    let empty: Value = json!([]);
    entity::ActiveModel {
        id: Set(LyricId::from_filename(filename).get().cast_signed()),
        filename: Set(filename.to_string()),
        source: Set("public".to_string()),
        timestamp: Set(0),
        track_names: Set(empty.clone()),
        artist_names: Set(empty.clone()),
        album_names: Set(empty.clone()),
        normalized_track_names: Set(empty.clone()),
        normalized_artist_names: Set(empty.clone()),
        normalized_album_names: Set(empty.clone()),
        ncm_music_ids: Set(empty.clone()),
        qq_music_ids: Set(empty.clone()),
        apple_music_ids: Set(empty.clone()),
        spotify_ids: Set(empty.clone()),
        isrcs: Set(empty.clone()),
        author_ids: Set(empty.clone()),
        author_usernames: Set(empty),
        lyric_text: Set(lyric_text.to_string()),
        bg_vocal_text: Set(bg_vocal_text.to_string()),
        raw_ttml: Set(raw_ttml.to_string()),
    }
}

async fn seeded_conn() -> DatabaseConnection {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    entity::Entity::insert(seed_row(
        "main_hit.ttml",
        "first line\nHello World Lyric\nlast line",
        "",
        "<tt/>",
    ))
    .exec(&conn)
    .await
    .expect("seed main row");

    entity::Entity::insert(seed_row(
        "bg_hit.ttml",
        "",
        "only in background vocal\nSunshine vocal",
        "<tt/>",
    ))
    .exec(&conn)
    .await
    .expect("seed bg row");

    entity::Entity::insert(seed_row("empty_text.ttml", "", "", "<tt/>"))
        .exec(&conn)
        .await
        .expect("seed empty row");

    conn
}

#[tokio::test]
async fn fts_match_in_main_lyric_yields_marked_snippet() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "Hello", 10).await.expect("fts query");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].field, LyricMatchField::MainLyric);
    assert_eq!(
        hits[0].snippet.as_deref(),
        Some("first line\n<mark>Hello</mark> World Lyric\nlast line")
    );
    assert_eq!(hits[0].id, LyricId::from_filename("main_hit.ttml"));
}

#[tokio::test]
async fn fts_match_falls_back_to_background_vocal() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "Sunshine", 10).await.expect("fts query");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].field, LyricMatchField::BackgroundVocal);
    assert_eq!(
        hits[0].snippet.as_deref(),
        Some("only in background vocal\n<mark>Sunshine</mark> vocal")
    );
}

#[tokio::test]
async fn fts_blank_keyword_returns_empty() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "   ", 10).await.expect("fts query");
    assert!(hits.is_empty());
}

#[tokio::test]
async fn fts_keyword_with_quotes_does_not_error() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "He\"llo", 10).await.expect("fts query");
    assert!(hits.is_empty());
}

#[tokio::test]
async fn fts_respects_limit() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "line", 1).await.expect("fts query");
    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn find_raw_ttml_hit_miss_and_empty() {
    let conn = seeded_conn().await;

    let found = find_raw_ttml(&conn, "main_hit.ttml").await;
    assert_eq!(found.as_deref(), Some("<tt/>"));

    let missing = find_raw_ttml(&conn, "nope.ttml").await;
    assert_eq!(missing, None);

    entity::Entity::insert(seed_row("blank.ttml", "x", "y", ""))
        .exec(&conn)
        .await
        .expect("seed blank row");
    let blank = find_raw_ttml(&conn, "blank.ttml").await;
    assert_eq!(blank, None);
}

#[tokio::test]
async fn fetch_song_entries_round_trips_metadata() {
    let conn = init_db("sqlite::memory:").await.expect("init db");

    let mut row = seed_row("meta.ttml", "lyric", "bg", "<tt/>");
    row.track_names = Set(json!(["Test Song One"]));
    row.artist_names = Set(json!(["Artist Alpha"]));
    row.normalized_track_names = Set(json!(["test song one"]));
    row.spotify_ids = Set(json!(["sp1001"]));
    row.timestamp = Set(1_600_000_000);
    entity::Entity::insert(row)
        .exec(&conn)
        .await
        .expect("seed row");

    let entries = fetch_song_entries(&conn).await.expect("fetch entries");
    assert_eq!(entries.len(), 1);

    let entry = &entries[0];
    assert_eq!(entry.id, LyricId::from_filename("meta.ttml"));
    assert_eq!(entry.filename.as_str(), "meta.ttml");
    assert_eq!(entry.timestamp, 1_600_000_000);
    assert_eq!(entry.track_names[0].as_str(), "Test Song One");
    assert_eq!(entry.artist_names[0].as_str(), "Artist Alpha");
    assert_eq!(entry.normalized_track_names[0].as_str(), "test song one");
    assert_eq!(entry.spotify_ids[0].as_str(), "sp1001");
}

#[tokio::test]
async fn fetch_song_entries_empty_table() {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let entries = fetch_song_entries(&conn).await.expect("fetch entries");
    assert!(entries.is_empty());
}

#[tokio::test]
async fn fetch_song_entries_maps_private_source() {
    use crate::core::models::LyricSource;

    let conn = init_db("sqlite::memory:").await.expect("init db");
    let mut row = seed_row("priv.ttml", "lyric", "bg", "<tt/>");
    row.source = Set("private".to_string());
    entity::Entity::insert(row)
        .exec(&conn)
        .await
        .expect("seed private row");

    let entries = fetch_song_entries(&conn).await.expect("fetch entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source, LyricSource::Private);
}
