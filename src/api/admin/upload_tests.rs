use super::*;
use crate::core::{
    db::{
        queries::fetch_song_entries,
        setup::init_db,
    },
    models::LyricSource,
};

const SAMPLE_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:00:01.000" end="00:00:02.000">hello</p></div></body></tt>"#;

#[test]
fn validate_filename_accepts_ttml() {
    assert_eq!(
        validate_filename("Artist - Title.ttml").unwrap(),
        "Artist - Title.ttml"
    );
    assert_eq!(validate_filename("  a.TTML  ").unwrap(), "a.TTML");
}

#[test]
fn validate_filename_rejects_bad_inputs() {
    for bad in [
        "",
        "   ",
        "song.lrc",
        "../evil.ttml",
        "a/b.ttml",
        "a\\b.ttml",
        &"x".repeat(300),
    ] {
        assert!(validate_filename(bad).is_err(), "should reject {bad:?}");
    }
}

fn bearer_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {token}").parse().expect("valid header"),
    );
    headers
}

#[tokio::test]
async fn authorize_admin_rejects_unconfigured_secret() {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let state = AppState::new_with_secret(conn, None);

    let err = authorize_admin(&state, &bearer_headers("s3cret")).unwrap_err();
    assert!(matches!(err, AppError::InternalServerError(_)));
}

#[tokio::test]
async fn authorize_admin_checks_bearer() {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let state = AppState::new_with_secret(conn, None).with_admin_secret("s3cret");

    assert!(authorize_admin(&state, &bearer_headers("s3cret")).is_ok());
    assert!(matches!(
        authorize_admin(&state, &bearer_headers("wrong")).unwrap_err(),
        AppError::Unauthorized
    ));
    assert!(matches!(
        authorize_admin(&state, &HeaderMap::new()).unwrap_err(),
        AppError::Unauthorized
    ));
}

#[tokio::test]
async fn upsert_private_lyric_round_trip() {
    let conn = init_db("sqlite::memory:").await.expect("init db");

    let id1 = upsert_private_lyric(&conn, "mine.ttml", SAMPLE_TTML)
        .await
        .expect("first upload");
    assert_eq!(id1, LyricId::from_private_filename("mine.ttml"));

    let updated = SAMPLE_TTML.replace("hello", "hello updated");
    let id2 = upsert_private_lyric(&conn, "mine.ttml", &updated)
        .await
        .expect("second upload");
    assert_eq!(id1, id2, "re-upload keeps the same id");

    let entries = fetch_song_entries(&conn).await.expect("fetch entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source, LyricSource::Private);
    assert_eq!(entries[0].id, id1);

    // lyric_text 应为更新后的内容
    let row = entity::Entity::find_by_id(id1.get().cast_signed())
        .one(&conn)
        .await
        .expect("query")
        .expect("row exists");
    assert!(row.lyric_text.contains("hello updated"));
}
