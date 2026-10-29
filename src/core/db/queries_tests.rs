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

fn no_filter() -> FtsFilter {
    FtsFilter::default()
}

#[tokio::test]
async fn fts_match_in_main_lyric_yields_main_field_without_snippet() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "Hello", &no_filter())
        .await
        .expect("fts query");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].field, LyricMatchField::MainLyric);
    assert_eq!(hits[0].id, LyricId::from_filename("main_hit.ttml"));
    assert_eq!(
        hits[0].snippet, None,
        "snippet 由 fetch_fts_snippets 按页补取"
    );
}

#[tokio::test]
async fn fts_match_only_in_background_vocal_yields_background_field() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "Sunshine", &no_filter())
        .await
        .expect("fts query");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].field, LyricMatchField::BackgroundVocal);
}

#[tokio::test]
async fn fts_blank_keyword_returns_empty() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "   ", &no_filter())
        .await
        .expect("fts query");
    assert!(hits.is_empty());
}

#[tokio::test]
async fn fts_keyword_with_quotes_does_not_error() {
    let conn = seeded_conn().await;
    let hits = search_fts(&conn, "He\"llo", &no_filter())
        .await
        .expect("fts query");
    assert!(hits.is_empty());
}

/// 过滤下推的夹具：来源与两个作者维度做成不对称的
///
/// | 文件 | source | authorId | authorUsername |
/// |---|---|---|---|
/// | `pub_a.ttml` | public | `a1` | `alice` |
/// | `pub_b.ttml` | public | `a2` | `bob` |
/// | `priv_a.ttml` | private | `a1` | `bob` |
async fn filter_conn() -> DatabaseConnection {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    for (filename, source, author_id, author_username) in [
        ("pub_a.ttml", "public", "a1", "alice"),
        ("pub_b.ttml", "public", "a2", "bob"),
        ("priv_a.ttml", "private", "a1", "bob"),
    ] {
        let mut row = seed_row(filename, "shared needle line", "", "<tt/>");
        if source == "private" {
            row.id = Set(LyricId::from_private_filename(filename).get().cast_signed());
        }
        row.source = Set(source.to_string());
        row.author_ids = Set(json!([author_id]));
        row.author_usernames = Set(json!([author_username]));
        entity::Entity::insert(row)
            .exec(&conn)
            .await
            .expect("seed filter row");
    }
    conn
}

fn hit_ids(hits: &[LyricHit]) -> Vec<LyricId> {
    let mut ids: Vec<LyricId> = hits.iter().map(|hit| hit.id).collect();
    ids.sort_unstable();
    ids
}

fn sorted(mut ids: Vec<LyricId>) -> Vec<LyricId> {
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn fts_pushes_source_and_author_filters_into_sql() {
    let conn = filter_conn().await;
    let pub_a = LyricId::from_filename("pub_a.ttml");
    let pub_b = LyricId::from_filename("pub_b.ttml");
    let priv_a = LyricId::from_private_filename("priv_a.ttml");

    let cases = [
        (no_filter(), vec![pub_a, pub_b, priv_a]),
        (
            FtsFilter {
                source: Some(LyricSource::Private),
                ..no_filter()
            },
            vec![priv_a],
        ),
        (
            FtsFilter {
                source: Some(LyricSource::Public),
                ..no_filter()
            },
            vec![pub_a, pub_b],
        ),
        (
            FtsFilter {
                author_id: Some("a1".to_string()),
                ..no_filter()
            },
            vec![pub_a, priv_a],
        ),
        (
            FtsFilter {
                author_username: Some("bob".to_string()),
                ..no_filter()
            },
            vec![pub_b, priv_a],
        ),
        // 各维度之间 AND
        (
            FtsFilter {
                author_id: Some("a1".to_string()),
                author_username: Some("bob".to_string()),
                ..no_filter()
            },
            vec![priv_a],
        ),
        (
            FtsFilter {
                source: Some(LyricSource::Public),
                author_id: Some("a1".to_string()),
                ..no_filter()
            },
            vec![pub_a],
        ),
        // 作者精确且大小写敏感，查不到即空集
        (
            FtsFilter {
                author_username: Some("Alice".to_string()),
                ..no_filter()
            },
            vec![],
        ),
        (
            FtsFilter {
                author_id: Some("nobody".to_string()),
                author_username: Some("alice".to_string()),
                ..no_filter()
            },
            vec![],
        ),
    ];

    for (filter, expected) in cases {
        let hits = search_fts(&conn, "needle", &filter)
            .await
            .expect("fts query");
        assert_eq!(hit_ids(&hits), sorted(expected), "filter: {filter:?}");
    }
}

#[tokio::test]
async fn fts_returns_every_match_without_window_truncation() {
    // 600 条超过旧实现的窗口上限 500
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let rows: Vec<entity::ActiveModel> = (0..600)
        .map(|i| seed_row(&format!("bulk_{i:03}.ttml"), "bulk needle", "", "<tt/>"))
        .collect();
    for chunk in rows.chunks(100) {
        entity::Entity::insert_many(chunk.to_vec())
            .exec(&conn)
            .await
            .expect("seed bulk rows");
    }

    let hits = search_fts(&conn, "needle", &no_filter())
        .await
        .expect("fts query");
    assert_eq!(hits.len(), 600);
    assert!(
        hits.windows(2).all(|w| w[0].rank <= w[1].rank),
        "命中按 bm25 升序给出"
    );
}

#[tokio::test]
async fn fetch_fts_snippets_marks_keyword_with_background_fallback() {
    let conn = seeded_conn().await;
    let main_id = LyricId::from_filename("main_hit.ttml");
    let bg_id = LyricId::from_filename("bg_hit.ttml");

    let main = fetch_fts_snippets(&conn, "Hello", &[main_id])
        .await
        .expect("snippet query");
    assert_eq!(
        main.get(&main_id).map(String::as_str),
        Some("first line\n<mark>Hello</mark> World Lyric\nlast line")
    );

    let bg = fetch_fts_snippets(&conn, "Sunshine", &[bg_id])
        .await
        .expect("snippet query");
    assert_eq!(
        bg.get(&bg_id).map(String::as_str),
        Some("only in background vocal\n<mark>Sunshine</mark> vocal")
    );
}

#[tokio::test]
async fn fetch_fts_snippets_reads_only_requested_ids() {
    let conn = seeded_conn().await;
    let main_id = LyricId::from_filename("main_hit.ttml");
    let empty_id = LyricId::from_filename("empty_text.ttml");

    // `line` 只在 main_hit 里；empty_text 正文为空，抽不出上下文就不出现在结果里
    let snippets = fetch_fts_snippets(&conn, "line", &[main_id, empty_id])
        .await
        .expect("snippet query");
    assert_eq!(snippets.len(), 1);
    assert!(snippets.contains_key(&main_id));

    let none = fetch_fts_snippets(&conn, "line", &[])
        .await
        .expect("snippet query");
    assert!(none.is_empty());

    let blank = fetch_fts_snippets(&conn, "  ", &[main_id])
        .await
        .expect("snippet query");
    assert!(blank.is_empty());
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
