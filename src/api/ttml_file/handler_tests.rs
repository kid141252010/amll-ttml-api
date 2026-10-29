use axum::{
    body::{
        Body,
        to_bytes,
    },
    http::{
        Request,
        StatusCode,
    },
};
use sea_orm::EntityTrait;
use tower::ServiceExt;

use super::*;
use crate::{
    core::db::{
        entity,
        setup::init_db,
    },
    create_app,
    services::sync_service::build_entity_from_ttml,
};

const SCRIPT_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:h="http://www.w3.org/1999/xhtml"><head><h:script>alert(1)</h:script></head><body><div><p begin="00:01.000" end="00:02.000">hello</p></div></body></tt>"#;

#[test]
fn resolves_supported_platforms() {
    assert!(matches!(
        resolve_platform("ncm-lyrics"),
        Ok(PlatformKind::Ncm)
    ));
    assert!(matches!(
        resolve_platform("qq-lyrics"),
        Ok(PlatformKind::Qq)
    ));
    assert!(matches!(
        resolve_platform("am-lyrics"),
        Ok(PlatformKind::Am)
    ));
    assert!(matches!(
        resolve_platform("spotify-lyrics"),
        Ok(PlatformKind::Spotify)
    ));
}

#[test]
fn rejects_unknown_platform() {
    // 注意：旧的瞎猜值（ncm/qq/apple/spotify）现在必须拒绝
    assert!(resolve_platform("ncm").is_err());
    assert!(resolve_platform("qq").is_err());
    assert!(resolve_platform("kugou").is_err());
    assert!(resolve_platform("").is_err());
}

#[test]
fn strips_ttml_suffix() {
    assert_eq!(strip_ttml_suffix("123.ttml"), Some("123"));
    assert_eq!(strip_ttml_suffix("abc123.ttml"), Some("abc123"));
}

#[test]
fn rejects_non_ttml_files() {
    assert_eq!(strip_ttml_suffix("123.xml"), None);
    assert_eq!(strip_ttml_suffix("123.ttm"), None);
    assert_eq!(strip_ttml_suffix("123.TTML"), None);
    assert_eq!(strip_ttml_suffix("123"), None);
}

#[test]
fn unknown_platform_message_follows_sentence_style() {
    assert!(matches!(
        resolve_platform("kugou"),
        Err(AppError::BadRequest(ref msg)) if msg == "Unsupported platform: 'kugou'."
    ));
}

mod route {
    use axum::{
        body::{
            Body,
            to_bytes,
        },
        http::{
            HeaderMap,
            Request,
            StatusCode,
            header::{
                CACHE_CONTROL,
                CONTENT_TYPE,
                ETAG,
            },
        },
    };
    use sea_orm::{
        DatabaseConnection,
        EntityTrait,
        IntoActiveModel,
    };
    use serde_json::json;
    use tower::ServiceExt;

    use crate::{
        AppState,
        api::shared::cache::{
            NOT_FOUND_CACHE_CONTROL,
            WEAK_CACHE_CONTROL,
        },
        core::{
            LyricId,
            db::entity,
        },
        create_app,
        init_db,
        services::sync_service::build_entity_from_ttml,
    };

    const PUBLIC_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">public</p></div></body></tt>"#;
    const PRIVATE_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">private</p></div></body></tt>"#;

    async fn seed(
        conn: &DatabaseConnection,
        filename: &str,
        raw: &str,
        ncm_id: &str,
        private: bool,
    ) {
        let parsed = ttml_processor::parse_ttml(raw).expect("sample ttml");
        let mut model = build_entity_from_ttml(filename, raw, &parsed);
        model.ncm_music_ids = json!([ncm_id]);
        if private {
            model.source = "private".to_string();
            model.id = LyricId::from_private_filename(filename).get().cast_signed();
        }
        entity::Entity::insert(model.into_active_model())
            .exec(conn)
            .await
            .expect("seed row");
    }

    /// 1001 公私两库都有，2002 只有公库
    async fn app() -> axum::Router {
        let conn = init_db("sqlite::memory:").await.expect("init db");
        seed(
            &conn,
            "1700000000000-public.ttml",
            PUBLIC_TTML,
            "1001",
            false,
        )
        .await;
        seed(&conn, "mine.ttml", PRIVATE_TTML, "1001", true).await;
        seed(
            &conn,
            "1700000000001-only-public.ttml",
            PUBLIC_TTML,
            "2002",
            false,
        )
        .await;
        let state = AppState::new_with_secret(conn, None);
        state.store.rebuild_index().await.expect("rebuild index");
        create_app(state)
    }

    async fn get(app: &axum::Router, uri: &str) -> (StatusCode, HeaderMap, String) {
        let response = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (
            status,
            headers,
            String::from_utf8(bytes.to_vec()).expect("utf-8"),
        )
    }

    #[tokio::test]
    async fn serves_raw_ttml_preferring_private_library() {
        let app = app().await;

        let (status, headers, body) = get(&app, "/ncm-lyrics/1001.ttml").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, PRIVATE_TTML);
        assert_eq!(headers[CONTENT_TYPE], "application/xml; charset=utf-8");
        assert_eq!(headers[CACHE_CONTROL], WEAK_CACHE_CONTROL);
        assert!(headers.contains_key(ETAG), "public 200 gets an ETag");

        let (status, _, body) = get(&app, "/ncm-lyrics/2002.ttml").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, PUBLIC_TTML);
    }

    #[tokio::test]
    async fn unknown_song_id_is_json_404() {
        let app = app().await;
        for uri in ["/ncm-lyrics/9999.ttml", "/qq-lyrics/1001.ttml"] {
            let (status, headers, body) = get(&app, uri).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
            assert_eq!(headers[CACHE_CONTROL], NOT_FOUND_CACHE_CONTROL, "{uri}");
            let json: serde_json::Value = serde_json::from_str(&body).expect("json");
            assert_eq!(json["status"], 404, "{uri}");
        }
    }

    #[tokio::test]
    async fn bad_suffix_is_404_and_unknown_platform_is_400() {
        let app = app().await;

        let (status, _, _) = get(&app, "/ncm-lyrics/1001.xml").await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _, body) = get(&app, "/kugou/1001.ttml").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let json: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(json["message"], "Unsupported platform: 'kugou'.");
    }
}

#[tokio::test]
async fn ttml_file_response_is_sandboxed_without_changing_body() {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let parsed = ttml_processor::parse_ttml(SCRIPT_TTML).expect("sample ttml");
    let mut model = build_entity_from_ttml("script.ttml", SCRIPT_TTML, &parsed);
    model.ncm_music_ids = serde_json::json!(["4242"]);
    entity::Entity::insert(entity::ActiveModel::from(model))
        .exec(&conn)
        .await
        .expect("seed row");
    let state = AppState::new_with_secret(conn, None);
    state.store.rebuild_index().await.expect("rebuild index");

    let response = create_app(state)
        .oneshot(
            Request::get("/ncm-lyrics/4242.ttml")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/xml; charset=utf-8"
    );
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(body, SCRIPT_TTML.as_bytes());
}
