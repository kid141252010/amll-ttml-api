use sea_orm::{
    ConnectionTrait,
    Database,
    EntityTrait,
};
use tempfile::NamedTempFile;

use super::{
    CREATE_LYRICS_TABLE_SQL,
    init_db,
    migrate_source_column,
};
use crate::core::db::entity;

const CREATE_OLD_LYRICS_TABLE_SQL: &str = r"
CREATE TABLE IF NOT EXISTS lyrics (
    id BIGINT PRIMARY KEY,
    filename TEXT NOT NULL UNIQUE,
    timestamp BIGINT NOT NULL,
    track_names JSON NOT NULL,
    artist_names JSON NOT NULL,
    album_names JSON NOT NULL,
    normalized_track_names JSON NOT NULL,
    normalized_artist_names JSON NOT NULL,
    normalized_album_names JSON NOT NULL,
    ncm_music_ids JSON NOT NULL,
    qq_music_ids JSON NOT NULL,
    apple_music_ids JSON NOT NULL,
    spotify_ids JSON NOT NULL,
    isrcs JSON NOT NULL,
    author_ids JSON NOT NULL,
    author_usernames JSON NOT NULL,
    lyric_text TEXT NOT NULL,
    bg_vocal_text TEXT NOT NULL,
    raw_ttml TEXT NOT NULL
);
";

#[tokio::test]
async fn migration_is_idempotent() {
    let file = NamedTempFile::new().unwrap();
    let db_url = format!("sqlite://{}?mode=rwc", file.path().display());

    init_db(&db_url).await.unwrap();
    // 重复执行不报错：ALTER 的 duplicate column 错误被吞掉，索引用 IF NOT EXISTS
    init_db(&db_url).await.unwrap();

    // 索引确实存在：去掉 IF NOT EXISTS 再建一次应报 already exists
    let db = Database::connect(&db_url).await.unwrap();
    let err = db
        .execute_unprepared(
            "CREATE UNIQUE INDEX idx_lyrics_source_filename ON lyrics(source, filename)",
        )
        .await
        .expect_err("index should already exist");
    assert!(err.to_string().contains("already exists"));
}

#[tokio::test]
async fn migration_backfills_public() {
    let file = NamedTempFile::new().unwrap();
    let db_url = format!("sqlite://{}?mode=rwc", file.path().display());
    let db = Database::connect(&db_url).await.unwrap();

    let _ = CREATE_LYRICS_TABLE_SQL;
    db.execute_unprepared(CREATE_OLD_LYRICS_TABLE_SQL)
        .await
        .unwrap();
    db.execute_unprepared(
        r"
            INSERT INTO lyrics (
                id, filename, timestamp, track_names, artist_names, album_names,
                normalized_track_names, normalized_artist_names, normalized_album_names,
                ncm_music_ids, qq_music_ids, apple_music_ids, spotify_ids, isrcs,
                author_ids, author_usernames, lyric_text, bg_vocal_text, raw_ttml
            ) VALUES (
                1, 'old.ttml', 0, '[]', '[]', '[]', '[]', '[]', '[]',
                '[]', '[]', '[]', '[]', '[]', '[]', '[]', '', '', ''
            )
            ",
    )
    .await
    .unwrap();

    migrate_source_column(&db).await.unwrap();

    let row = entity::Entity::find()
        .one(&db)
        .await
        .unwrap()
        .expect("row exists");
    assert_eq!(row.source, "public");
}
