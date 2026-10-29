use std::{
    fmt::Write,
    sync::Arc,
};

use axum::{
    body::{
        Body,
        to_bytes,
    },
    http::{
        Request,
        StatusCode,
        header::CONTENT_TYPE,
    },
};
use sea_orm::ConnectionTrait;
use serde_json::Value;
use tower::ServiceExt;

use super::*;
use crate::{
    core::{
        db::{
            queries::fetch_song_entries,
            setup::init_db,
        },
        models::LyricSource,
    },
    create_app,
};

const SAMPLE_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:amll="http://www.example.com/ns/amll">
        <head><metadata>
            <amll:meta key="appleMusicId" value="1540793097"/>
            <amll:meta key="appleMusicId" value="1540793098"/>
            <amll:meta key="appleMusicId" value="1540793097"/>
            <amll:meta key="spotifyId" value="sp1001"/>
            <amll:meta key="ncmMusicId" value="1001"/>
            <amll:meta key="qqMusicId" value="001abc"/>
            <amll:meta key="isrc" value="US123"/>
        </metadata></head>
        <body><div><p begin="00:00:01.000" end="00:00:02.000">hello</p></div></body>
    </tt>"#;

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
    assert_eq!(
        row.apple_music_ids,
        serde_json::json!(["1540793097", "1540793098"])
    );
    assert_eq!(row.spotify_ids, serde_json::json!(["sp1001"]));
    assert_eq!(row.ncm_music_ids, serde_json::json!(["1001"]));
    assert_eq!(row.qq_music_ids, serde_json::json!(["001abc"]));
    assert_eq!(row.isrcs, serde_json::json!(["US123"]));
}

async fn batch_state() -> AppState {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    AppState::new_with_secret(conn, None).with_admin_secret("s3cret")
}

fn batch_file(filename: &str, content: &str) -> (String, Vec<u8>) {
    (filename.to_string(), content.as_bytes().to_vec())
}

#[tokio::test]
async fn batch_upload_saves_private_lyrics_in_request_order() {
    let state = batch_state().await;
    state
        .store
        .ttml_cache
        .insert("second.ttml".to_string(), "old content".to_string())
        .await;

    let data = process_batch_upload(
        &state,
        vec![
            batch_file("second.ttml", SAMPLE_TTML),
            batch_file("first.ttml", SAMPLE_TTML),
        ],
    )
    .await
    .expect("batch upload");

    assert_eq!((data.succeeded, data.failed), (2, 0));
    assert_eq!(data.results.len(), 2);
    for (result, filename) in data.results.iter().zip(["second.ttml", "first.ttml"]) {
        assert_eq!(result.filename, filename);
        assert_eq!(
            result.id,
            Some(LyricId::from_private_filename(filename).get())
        );
        assert!(result.error.is_none());
    }
    let entries = fetch_song_entries(&state.store.conn)
        .await
        .expect("entries");
    assert_eq!(entries.len(), 2);
    for result in &data.results {
        assert!(entries.iter().any(|entry| {
            entry.filename.as_str() == result.filename
                && Some(entry.id.get()) == result.id
                && entry.source == LyricSource::Private
        }));
    }
    assert_eq!(
        state.store.lyric_count(),
        2,
        "uploads are indexed immediately"
    );
    assert!(state.store.ttml_cache.get("second.ttml").await.is_none());
}

#[tokio::test]
async fn batch_upload_continues_after_invalid_ttml() {
    let state = batch_state().await;
    let data = process_batch_upload(
        &state,
        vec![
            batch_file("invalid.ttml", "<tt"),
            batch_file("valid.ttml", SAMPLE_TTML),
        ],
    )
    .await
    .expect("partial success");

    assert_eq!((data.succeeded, data.failed), (1, 1));
    assert_eq!(data.results[0].filename, "invalid.ttml");
    assert!(data.results[0].id.is_none());
    assert!(
        data.results[0]
            .error
            .as_ref()
            .unwrap()
            .contains("Invalid TTML")
    );
    assert_eq!(data.results[1].filename, "valid.ttml");
    assert!(data.results[1].id.is_some());
    assert!(data.results[1].error.is_none());
    let entries = fetch_song_entries(&state.store.conn)
        .await
        .expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].filename.as_str(), "valid.ttml");
    assert_eq!(entries[0].source, LyricSource::Private);
    assert_eq!(state.store.lyric_count(), 1);
}

#[tokio::test]
async fn batch_upload_continues_after_public_filename_conflict() {
    let state = batch_state().await;
    let parsed = ttml_processor::parse_ttml(SAMPLE_TTML).expect("sample ttml");
    let mut public = build_entity_from_ttml("public.ttml", SAMPLE_TTML, &parsed);
    public.source = "public".to_string();
    entity::Entity::insert(entity::ActiveModel::from(public))
        .exec(&state.store.conn)
        .await
        .expect("seed public row");

    let data = process_batch_upload(
        &state,
        vec![
            batch_file("public.ttml", SAMPLE_TTML),
            batch_file("private.ttml", SAMPLE_TTML),
        ],
    )
    .await
    .expect("partial success");

    assert_eq!((data.succeeded, data.failed), (1, 1));
    assert_eq!(data.results[0].filename, "public.ttml");
    assert!(data.results[0].id.is_none());
    assert_eq!(
        data.results[0].error.as_deref(),
        Some("filename already exists in the public library")
    );
    assert_eq!(data.results[1].filename, "private.ttml");
    assert!(data.results[1].error.is_none());
    let entries = fetch_song_entries(&state.store.conn)
        .await
        .expect("entries");
    assert_eq!(entries.len(), 2);
    assert!(
        entries.iter().any(|entry| {
            entry.filename == "public.ttml" && entry.source == LyricSource::Public
        })
    );
    assert!(
        entries.iter().any(|entry| {
            entry.filename == "private.ttml" && entry.source == LyricSource::Private
        })
    );
}

#[tokio::test]
async fn batch_upload_rejects_empty_list() {
    let state = batch_state().await;
    let error = process_batch_upload(&state, vec![]).await.unwrap_err();
    assert!(matches!(error, AppError::BadRequest(_)));
}

#[tokio::test]
async fn batch_upload_checks_filename_and_utf8_before_saving() {
    let state = batch_state().await;
    let data = process_batch_upload(
        &state,
        vec![
            batch_file("../bad.ttml", SAMPLE_TTML),
            ("encoding.ttml".to_string(), vec![0xff]),
            batch_file("valid.ttml", SAMPLE_TTML),
        ],
    )
    .await
    .expect("partial success");

    assert_eq!((data.succeeded, data.failed), (1, 2));
    assert_eq!(
        data.results[0].error.as_deref(),
        Some("filename must not contain path separators")
    );
    assert_eq!(
        data.results[1].error.as_deref(),
        Some("file is not valid UTF-8")
    );
    assert!(data.results[2].id.is_some());
    assert_eq!(state.store.lyric_count(), 1);
}

#[tokio::test]
async fn batch_upload_preserves_index_and_caches_when_all_files_fail() {
    let state = batch_state().await;
    let index = state.store.index.load_full();
    state
        .store
        .ttml_cache
        .insert("cached.ttml".to_string(), "cached".to_string())
        .await;

    let data = process_batch_upload(&state, vec![batch_file("invalid.ttml", "<tt")])
        .await
        .expect("per-file failure");

    assert_eq!((data.succeeded, data.failed), (0, 1));
    assert!(Arc::ptr_eq(&index, &state.store.index.load_full()));
    assert_eq!(
        state.store.ttml_cache.get("cached.ttml").await.as_deref(),
        Some("cached")
    );
}

#[tokio::test]
async fn batch_upload_enforces_file_count_before_writing() {
    let state = batch_state().await;
    let files = (0..=MAX_BATCH_FILES)
        .map(|i| batch_file(&format!("{i}.ttml"), SAMPLE_TTML))
        .collect();
    let error = process_batch_upload(&state, files).await.unwrap_err();
    assert!(matches!(error, AppError::BadRequest(_)));
    assert!(
        fetch_song_entries(&state.store.conn)
            .await
            .unwrap()
            .is_empty()
    );

    let files = (0..MAX_BATCH_FILES)
        .map(|i| batch_file(&format!("{i}.ttml"), SAMPLE_TTML))
        .collect();
    let data = process_batch_upload(&state, files)
        .await
        .expect("100 files allowed");
    assert_eq!((data.succeeded, data.failed), (MAX_BATCH_FILES, 0));
    assert_eq!(state.store.lyric_count(), MAX_BATCH_FILES);
}

#[tokio::test]
async fn batch_upload_reports_index_rebuild_failure_after_saving() {
    let state = batch_state().await;
    upsert_private_lyric(&state.store.conn, "broken.ttml", SAMPLE_TTML)
        .await
        .expect("seed row");
    // 模拟已有行的元数据损坏：新文件仍能落库，但全量索引重建失败
    state
        .store
        .conn
        .execute_unprepared("UPDATE lyrics SET track_names = 'invalid json'")
        .await
        .expect("corrupt metadata");

    let error = process_batch_upload(&state, vec![batch_file("saved.ttml", SAMPLE_TTML)])
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::InternalServerError(message)
            if message == "Lyric saved but index rebuild failed"
    ));
    let row = entity::Entity::find_by_id(
        LyricId::from_private_filename("saved.ttml")
            .get()
            .cast_signed(),
    )
    .one(&state.store.conn)
    .await
    .expect("query")
    .expect("file was saved");
    assert_eq!(row.source, "private");
    assert_eq!(row.raw_ttml, SAMPLE_TTML);
}

fn multipart_body(fields: &[(&str, Option<&str>, &str)]) -> String {
    let mut body = String::new();
    for (name, filename, content) in fields {
        let filename = filename.map_or_else(String::new, |name| format!("; filename=\"{name}\""));
        write!(
            body,
            "--batch-boundary\r\nContent-Disposition: form-data; name=\"{name}\"{filename}\r\n\r\n{content}"
        )
        .expect("write multipart part");
        body.push_str("\r\n");
    }
    body.push_str("--batch-boundary--\r\n");
    body
}

fn multipart_request(uri: &str, fields: &[(&str, Option<&str>, &str)]) -> Request<Body> {
    Request::post(uri)
        .header(AUTHORIZATION, "Bearer s3cret")
        .header(CONTENT_TYPE, "multipart/form-data; boundary=batch-boundary")
        .body(Body::from(multipart_body(fields)))
        .expect("request")
}

async fn upload_response(state: &AppState, request: Request<Body>) -> (StatusCode, Value) {
    let response = create_app(state.clone())
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.headers()[CACHE_CONTROL], NO_STORE_CACHE_CONTROL);
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, serde_json::from_slice(&bytes).expect("json"))
}

#[tokio::test]
async fn batch_route_uses_part_filenames_and_envelope_under_both_prefixes() {
    let state = batch_state().await;
    for prefix in crate::API_PREFIXES {
        let request = multipart_request(
            &format!("{prefix}/admin/lyrics/batch"),
            &[
                ("filename", None, "ignored.ttml"),
                ("file", Some("valid.ttml"), SAMPLE_TTML),
                ("other", Some("ignored.ttml"), SAMPLE_TTML),
                ("file", Some("invalid.ttml"), "<tt"),
            ],
        );
        let (status, json) = upload_response(&state, request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], 200);
        let data = &json["data"];
        assert_eq!(data["succeeded"], 1);
        assert_eq!(data["failed"], 1);
        assert_eq!(data["results"].as_array().unwrap().len(), 2);
        assert_eq!(data["results"][0]["filename"], "valid.ttml");
        assert_eq!(
            data["results"][0]["id"],
            LyricId::from_private_filename("valid.ttml").get()
        );
        assert!(data["results"][0]["error"].is_null());
        assert_eq!(data["results"][1]["filename"], "invalid.ttml");
        assert!(data["results"][1]["id"].is_null());
        assert!(
            data["results"][1]["error"]
                .as_str()
                .unwrap()
                .contains("Invalid TTML")
        );
    }
    assert_eq!(state.store.lyric_count(), 1);
}

#[tokio::test]
async fn batch_route_requires_admin_secret() {
    let state = batch_state().await;
    for token in [None, Some("Bearer wrong")] {
        let mut request = multipart_request(
            "/v1/admin/lyrics/batch",
            &[("file", Some("a.ttml"), SAMPLE_TTML)],
        );
        request.headers_mut().remove(AUTHORIZATION);
        if let Some(token) = token {
            request
                .headers_mut()
                .insert(AUTHORIZATION, token.parse().unwrap());
        }
        let (status, _) = upload_response(&state, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let mut unconfigured = state.clone();
    unconfigured.admin_secret = None;
    let request = multipart_request(
        "/v1/admin/lyrics/batch",
        &[("file", Some("a.ttml"), SAMPLE_TTML)],
    );
    let (status, _) = upload_response(&unconfigured, request).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        fetch_song_entries(&state.store.conn)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn batch_route_rejects_missing_files_and_malformed_multipart() {
    let state = batch_state().await;
    let request = multipart_request(
        "/v1/admin/lyrics/batch",
        &[("filename", None, "ignored.ttml")],
    );
    let (status, _) = upload_response(&state, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let request = Request::post("/v1/admin/lyrics/batch")
        .header(AUTHORIZATION, "Bearer s3cret")
        .body(Body::empty())
        .unwrap();
    let (status, _) = upload_response(&state, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut request = multipart_request("/v1/admin/lyrics/batch", &[]);
    *request.body_mut() = Body::from(format!(
        "--batch-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.ttml\"\r\n\r\n{SAMPLE_TTML}\r\n"
    ));
    let (status, _) = upload_response(&state, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        fetch_song_entries(&state.store.conn)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn batch_route_handles_missing_part_filename_as_file_error() {
    let state = batch_state().await;
    let request = multipart_request(
        "/v1/admin/lyrics/batch",
        &[
            ("file", None, SAMPLE_TTML),
            ("file", Some("valid.ttml"), SAMPLE_TTML),
        ],
    );
    let (status, json) = upload_response(&state, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["succeeded"], 1);
    assert_eq!(json["data"]["failed"], 1);
    assert_eq!(
        json["data"]["results"][0]["error"],
        "filename must not be empty"
    );
    assert_eq!(state.store.lyric_count(), 1);
}

#[tokio::test]
async fn batch_route_rejects_over_100_files_without_writing() {
    let state = batch_state().await;
    let fields = vec![("file", Some("valid.ttml"), SAMPLE_TTML); MAX_BATCH_FILES + 1];
    let request = multipart_request("/v1/admin/lyrics/batch", &fields);
    let (status, _) = upload_response(&state, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        fetch_song_entries(&state.store.conn)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn batch_route_relaxes_body_limit_without_changing_single_upload() {
    let state = batch_state().await;
    let large_ttml = format!("<!--{}-->{SAMPLE_TTML}", "x".repeat(2 * 1024 * 1024));
    let fields = [
        ("filename", None, "large.ttml"),
        ("file", Some("large.ttml"), large_ttml.as_str()),
    ];
    let request = multipart_request("/v1/admin/lyrics", &fields);
    let (status, _) = upload_response(&state, request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        fetch_song_entries(&state.store.conn)
            .await
            .unwrap()
            .is_empty()
    );

    let request = multipart_request("/v1/admin/lyrics/batch", &fields);
    let (status, json) = upload_response(&state, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["data"]["succeeded"], 1);
    assert_eq!(state.store.lyric_count(), 1);
}
