use sea_orm::{
    EntityTrait,
    Set,
};
use serde_json::{
    Value,
    json,
};

use super::*;
use crate::core::{
    LyricId,
    db::{
        entity,
        setup::init_db,
    },
    models::IdQuery,
};

#[tokio::test]
async fn rebuild_index_makes_sqlite_rows_queryable() {
    let conn = init_db("sqlite::memory:").await.expect("init db");

    let empty: Value = json!([]);
    entity::Entity::insert(entity::ActiveModel {
        id: Set(LyricId::from_filename("one.ttml").get().cast_signed()),
        filename: Set("one.ttml".to_string()),
        source: Set("public".to_string()),
        timestamp: Set(1),
        track_names: Set(json!(["Test Song One"])),
        artist_names: Set(json!(["Artist Alpha"])),
        album_names: Set(empty.clone()),
        normalized_track_names: Set(json!(["test song one"])),
        normalized_artist_names: Set(json!(["artist alpha"])),
        normalized_album_names: Set(empty.clone()),
        ncm_music_ids: Set(empty.clone()),
        qq_music_ids: Set(empty.clone()),
        apple_music_ids: Set(empty.clone()),
        spotify_ids: Set(json!(["sp1001"])),
        isrcs: Set(empty.clone()),
        author_ids: Set(empty.clone()),
        author_usernames: Set(empty),
        lyric_text: Set(String::new()),
        bg_vocal_text: Set(String::new()),
        raw_ttml: Set(String::new()),
    })
    .exec(&conn)
    .await
    .expect("seed row");

    let store = DbLyricStore::new(conn);
    store.rebuild_index().await.expect("rebuild index");

    let index = store.index.load_full();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].filename.as_str(), "one.ttml");

    let hits = index.find_by_ids(&IdQuery {
        spotify_ids: vec!["sp1001".to_string()],
        ..Default::default()
    });
    assert_eq!(hits, vec![0]);
}
