use serde_json::json;

use super::*;

#[test]
fn test_build_entity_from_ttml_basic_metadata() {
    let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:amll="http://www.example.com/ns/amll">
        <head>
            <metadata>
                <amll:meta key="musicName" value="Test Song" />
                <amll:meta key="artists" value="Test Artist" />
                <amll:meta key="album" value="Test Album" />
                <amll:meta key="ttmlAuthorGithub" value="999" />
                <amll:meta key="ttmlAuthorGithubLogin" value="dev_user" />
            </metadata>
        </head>
        <body>
            <div>
                <p begin="00:01.000" end="00:03.000">Hello world</p>
            </div>
        </body>
        </tt>"#;

    let parsed = ttml_processor::parse_ttml(ttml).expect("parse ttml");
    let model = build_entity_from_ttml("1689087424000-999-abc.ttml", ttml, &parsed);

    assert_eq!(model.track_names, json!(["Test Song"]));
    assert_eq!(model.artist_names, json!(["Test Artist"]));
    assert_eq!(model.album_names, json!(["Test Album"]));
    assert_eq!(model.author_ids, json!(["999"]));
    assert_eq!(model.author_usernames, json!(["dev_user"]));
    assert_eq!(model.source, "public");
    // Platform IDs are initialized to empty in build_entity_from_ttml, as they are sourced from
    // index.jsonl
    assert_eq!(model.ncm_music_ids, json!([]));
    assert_eq!(model.qq_music_ids, json!([]));
    assert_eq!(model.spotify_ids, json!([]));
    assert_eq!(model.apple_music_ids, json!([]));
    assert_eq!(model.isrcs, json!([]));
}

#[test]
fn test_merge_raw_index_entry_binds_all_platform_ids() {
    let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><head><metadata></metadata></head><body><div><p begin="00:01.000" end="00:02.000">Text</p></div></body></tt>"#;
    let parsed = ttml_processor::parse_ttml(ttml).expect("parse ttml");
    let mut model = build_entity_from_ttml("1689087424000-999-abc.ttml", ttml, &parsed);

    assert_eq!(model.ncm_music_ids, json!([]));
    assert_eq!(model.qq_music_ids, json!([]));
    assert_eq!(model.apple_music_ids, json!([]));
    assert_eq!(model.spotify_ids, json!([]));
    assert_eq!(model.isrcs, json!([]));

    let raw = RawIndexEntry {
        raw_lyric_file: "1689087424000-999-abc.ttml".to_string(),
        metadata: vec![
            ("ncmMusicId".to_string(), vec!["1001".to_string()]),
            ("qqMusicId".to_string(), vec!["001abc".to_string()]),
            ("appleMusicId".to_string(), vec!["am123".to_string()]),
            ("spotifyId".to_string(), vec!["sp1001".to_string()]),
            ("isrc".to_string(), vec!["US123".to_string()]),
        ],
    };

    merge_raw_index_entry(&mut model, raw);

    assert_eq!(model.ncm_music_ids, json!(["1001"]));
    assert_eq!(model.qq_music_ids, json!(["001abc"]));
    assert_eq!(model.apple_music_ids, json!(["am123"]));
    assert_eq!(model.spotify_ids, json!(["sp1001"]));
    assert_eq!(model.isrcs, json!(["US123"]));
}

#[test]
fn test_merge_ttml_platform_ids_binds_and_deduplicates_all_ids() {
    let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">Text</p></div></body></tt>"#;
    let parsed = ttml_processor::parse_ttml(ttml).expect("parse ttml");
    let mut model = build_entity_from_ttml("1689087424000-999-abc.ttml", ttml, &parsed);
    let metadata = TTMLMetadata {
        platform_ids: Some(
            [
                (PlatformId::NcmMusicId, ["1002", "1001", "1002"]),
                (PlatformId::QqMusicId, ["002def", "001abc", "002def"]),
                (PlatformId::AppleMusicId, ["am456", "am123", "am456"]),
                (PlatformId::SpotifyId, ["sp1002", "sp1001", "sp1002"]),
            ]
            .into_iter()
            .map(|(platform, ids)| (platform, ids.into_iter().map(str::to_string).collect()))
            .collect(),
        ),
        isrc: Some(vec![
            "US456".to_string(),
            "US123".to_string(),
            "US456".to_string(),
        ]),
        ..Default::default()
    };
    let empty_lists = TTMLMetadata {
        platform_ids: metadata
            .platform_ids
            .as_ref()
            .map(|ids| ids.keys().map(|key| (key.clone(), Vec::new())).collect()),
        isrc: Some(Vec::new()),
        ..Default::default()
    };

    for metadata in [&metadata, &TTMLMetadata::default(), &empty_lists] {
        merge_ttml_platform_ids(&mut model, metadata);

        assert_eq!(model.ncm_music_ids, json!(["1002", "1001"]));
        assert_eq!(model.qq_music_ids, json!(["002def", "001abc"]));
        assert_eq!(model.apple_music_ids, json!(["am456", "am123"]));
        assert_eq!(model.spotify_ids, json!(["sp1002", "sp1001"]));
        assert_eq!(model.isrcs, json!(["US456", "US123"]));
    }
}

#[test]
fn test_merge_ttml_platform_ids_empty_metadata_keeps_empty_ids() {
    let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">Text</p></div></body></tt>"#;
    let parsed = ttml_processor::parse_ttml(ttml).expect("parse ttml");
    let mut model = build_entity_from_ttml("1689087424000-999-abc.ttml", ttml, &parsed);

    merge_ttml_platform_ids(&mut model, &parsed.metadata);

    assert_eq!(model.ncm_music_ids, json!([]));
    assert_eq!(model.qq_music_ids, json!([]));
    assert_eq!(model.apple_music_ids, json!([]));
    assert_eq!(model.spotify_ids, json!([]));
    assert_eq!(model.isrcs, json!([]));
}

#[tokio::test]
async fn public_upsert_never_overwrites_private_row() {
    use crate::core::db::setup::init_db;

    let conn = init_db("sqlite::memory:").await.expect("init db");
    let private_model = entity::ActiveModel {
        id: Set(LyricId::from_private_filename("shared.ttml")
            .get()
            .cast_signed()),
        filename: Set("shared.ttml".to_string()),
        timestamp: Set(1),
        source: Set("private".to_string()),
        track_names: Set(json!([])),
        artist_names: Set(json!([])),
        album_names: Set(json!([])),
        normalized_track_names: Set(json!([])),
        normalized_artist_names: Set(json!([])),
        normalized_album_names: Set(json!([])),
        ncm_music_ids: Set(json!([])),
        qq_music_ids: Set(json!([])),
        apple_music_ids: Set(json!([])),
        spotify_ids: Set(json!([])),
        isrcs: Set(json!([])),
        author_ids: Set(json!([])),
        author_usernames: Set(json!([])),
        lyric_text: Set("private lyric".to_string()),
        bg_vocal_text: Set(String::new()),
        raw_ttml: Set("<tt>private</tt>".to_string()),
    };
    entity::Entity::insert(private_model)
        .exec(&conn)
        .await
        .expect("seed private row");

    let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">public text</p></div></body></tt>"#;
    let parsed = ttml_processor::parse_ttml(ttml).expect("parse ttml");
    let colliding = build_entity_from_ttml("shared.ttml", ttml, &parsed);
    let normal = build_entity_from_ttml("public-only.ttml", ttml, &parsed);
    let svc = SyncService::new(conn.clone(), reqwest::Client::new());
    svc.batch_upsert_entries(&[colliding, normal])
        .await
        .expect("upsert");

    // 私库行未被覆盖
    let row = entity::Entity::find_by_id(
        LyricId::from_private_filename("shared.ttml")
            .get()
            .cast_signed(),
    )
    .one(&conn)
    .await
    .expect("query")
    .expect("row exists");
    assert_eq!(row.source, "private");
    assert_eq!(row.lyric_text, "private lyric");

    // 同批不冲突的公库行正常写入
    let public_row = entity::Entity::find()
        .filter(entity::Column::Filename.eq("public-only.ttml"))
        .one(&conn)
        .await
        .expect("query")
        .expect("public row exists");
    assert_eq!(public_row.source, "public");
}
