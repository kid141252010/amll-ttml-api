use axum::{
    Router,
    body::{
        Body,
        to_bytes,
    },
    http::{
        Request,
        StatusCode,
    },
};
use sea_orm::{
    EntityTrait,
    IntoActiveModel,
};
use serde_json::Value;
use tower::ServiceExt;

use crate::{
    core::{
        LyricId,
        db::{
            entity,
            setup::init_db,
        },
    },
    create_app,
    services::sync_service::build_entity_from_ttml,
};

const PUBLIC_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:03.000">Public Lyric</p></div></body></tt>"#;
const PRIVATE_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:03.000">Private Lyric</p></div></body></tt>"#;

/// 公私两库各一首同名同歌手的《Hello》-Adele，匹配同为 Perfect
async fn same_song_app() -> Router {
    let db_conn = init_db("sqlite::memory:").await.expect("init in-memory db");

    for (filename, raw, private) in [
        ("pub_hello.ttml", PUBLIC_TTML, false),
        ("priv_hello.ttml", PRIVATE_TTML, true),
    ] {
        let parsed = ttml_processor::parse_ttml(raw).expect("sample ttml must parse");
        let mut model = build_entity_from_ttml(filename, raw, &parsed);
        model.track_names = serde_json::json!(["Hello"]);
        model.normalized_track_names = serde_json::json!(["hello"]);
        model.artist_names = serde_json::json!(["Adele"]);
        model.normalized_artist_names = serde_json::json!(["adele"]);
        if private {
            model.source = "private".to_string();
            model.id = LyricId::from_private_filename(filename).get().cast_signed();
        }
        entity::Entity::insert(model.into_active_model())
            .exec(&db_conn)
            .await
            .expect("seed entity row");
    }

    let state = crate::AppState::new_with_secret(db_conn, None);
    state.store.rebuild_index().await.expect("rebuild index");
    create_app(state)
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::get(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router call is infallible");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    (status, serde_json::from_slice(&bytes).expect("json body"))
}

const HELLO_ADELE: &str = "/v1/lrclib/get?track_name=Hello&artist_name=Adele";

#[tokio::test]
async fn lrclib_get_same_score_defaults_to_private() {
    let app = same_song_app().await;
    let (status, body) = get_json(&app, HELLO_ADELE).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["plainLyrics"], "Private Lyric");
}

#[tokio::test]
async fn lrclib_get_with_source_private_returns_private() {
    let app = same_song_app().await;
    let (status, body) = get_json(&app, &format!("{HELLO_ADELE}&source=private")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["id"],
        LyricId::from_private_filename("priv_hello.ttml").get()
    );
    assert_eq!(body["plainLyrics"], "Private Lyric");
}

#[tokio::test]
async fn lrclib_get_with_source_public_returns_public() {
    let app = same_song_app().await;
    let (status, body) = get_json(&app, &format!("{HELLO_ADELE}&source=public")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], LyricId::from_filename("pub_hello.ttml").get());
    assert_eq!(body["plainLyrics"], "Public Lyric");
}
