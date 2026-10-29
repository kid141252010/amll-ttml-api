use std::collections::HashSet;

use sea_orm::{
    EntityTrait,
    Set,
};
use serde_json::{
    Value,
    json,
};

use super::*;
use crate::{
    core::{
        LyricId,
        db::{
            entity,
            setup::init_db,
        },
        models::{
            IdQuery,
            LyricSource,
            SearchQuery,
        },
        pagination::Pagination,
    },
    services::lyric_service::search_lyric,
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

// --- /lyrics/search 正文检索：过滤与 total 作用在完整命中集上（真实 SQLite） ---

const BULK_PUBLIC: usize = 520;
const TARGET_PUBLIC: usize = 5;
const PRIVATE: usize = 3;
const TOTAL: usize = BULK_PUBLIC + TARGET_PUBLIC + PRIVATE;

fn fts_row(
    filename: &str,
    source: LyricSource,
    author_id: &str,
    lyric_text: &str,
) -> entity::ActiveModel {
    let empty: Value = json!([]);
    let id = match source {
        LyricSource::Public => LyricId::from_filename(filename),
        LyricSource::Private => LyricId::from_private_filename(filename),
    };
    entity::ActiveModel {
        id: Set(id.get().cast_signed()),
        filename: Set(filename.to_string()),
        source: Set(source.as_str().to_string()),
        timestamp: Set(1),
        track_names: Set(json!([filename])),
        artist_names: Set(empty.clone()),
        album_names: Set(empty.clone()),
        normalized_track_names: Set(json!([filename])),
        normalized_artist_names: Set(empty.clone()),
        normalized_album_names: Set(empty.clone()),
        ncm_music_ids: Set(empty.clone()),
        qq_music_ids: Set(empty.clone()),
        apple_music_ids: Set(empty.clone()),
        spotify_ids: Set(empty.clone()),
        isrcs: Set(empty.clone()),
        author_ids: Set(json!([author_id])),
        author_usernames: Set(empty),
        lyric_text: Set(lyric_text.to_string()),
        bg_vocal_text: Set(String::new()),
        raw_ttml: Set(String::new()),
    }
}

/// 正文同词命中超过旧窗口上限（500）的库
///
/// 公库大头词频高、文本短，bm25 排在最前；私库与 `target` 作者的条目文本长、
/// 词频低，bm25 排在最后——正好落在旧实现「全库 ORDER BY rank LIMIT 窗口」之外
async fn crowded_fts_store() -> DbLyricStore {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let long_text = format!("needle\n{}", "filler words here ".repeat(60));

    let mut rows: Vec<entity::ActiveModel> = (0..BULK_PUBLIC)
        .map(|i| {
            fts_row(
                &format!("bulk_{i:03}.ttml"),
                LyricSource::Public,
                "other",
                "needle needle needle",
            )
        })
        .collect();
    rows.extend((0..TARGET_PUBLIC).map(|i| {
        fts_row(
            &format!("target_{i}.ttml"),
            LyricSource::Public,
            "target",
            &long_text,
        )
    }));
    rows.extend((0..PRIVATE).map(|i| {
        fts_row(
            &format!("mine_{i}.ttml"),
            LyricSource::Private,
            "other",
            &long_text,
        )
    }));
    for chunk in rows.chunks(100) {
        entity::Entity::insert_many(chunk.to_vec())
            .exec(&conn)
            .await
            .expect("seed rows");
    }

    let store = DbLyricStore::new(conn);
    store.rebuild_index().await.expect("rebuild index");
    store
}

fn lyric_query(source: Option<LyricSource>, author_id: Option<&str>) -> SearchQuery {
    SearchQuery {
        lyric_text: Some("needle".to_string()),
        source,
        author_id: author_id.map(str::to_owned),
        ..Default::default()
    }
}

const fn page(page: u64, page_size: u64) -> Pagination {
    Pagination { page, page_size }
}

#[tokio::test]
async fn lyric_search_private_source_returns_every_private_hit() {
    let store = crowded_fts_store().await;

    let res = search_lyric(
        &store,
        &lyric_query(Some(LyricSource::Private), None),
        page(1, 50),
    )
    .await;

    assert_eq!(res.pagination.total, PRIVATE as u64);
    assert_eq!(res.items.len(), PRIVATE);
    assert!(
        res.items
            .iter()
            .all(|hit| hit.entry.source == LyricSource::Private)
    );
    // snippet 在分页后按页补取，私库条目同样要带上
    assert!(res.items.iter().all(|hit| {
        hit.lyric_hit
            .as_ref()
            .and_then(|l| l.snippet.as_deref())
            .is_some_and(|s| s.contains("<mark>needle</mark>"))
    }));
}

#[tokio::test]
async fn lyric_search_public_source_excludes_private_hits() {
    let store = crowded_fts_store().await;

    let res = search_lyric(
        &store,
        &lyric_query(Some(LyricSource::Public), None),
        page(1, 100),
    )
    .await;

    assert_eq!(res.pagination.total, (BULK_PUBLIC + TARGET_PUBLIC) as u64);
    assert!(
        res.items
            .iter()
            .all(|hit| hit.entry.source == LyricSource::Public)
    );
}

#[tokio::test]
async fn lyric_search_all_sources_reports_true_total_and_every_page_is_reachable() {
    let store = crowded_fts_store().await;
    let query = lyric_query(None, None);

    let first = search_lyric(&store, &query, page(1, 100)).await;
    assert_eq!(first.pagination.total, TOTAL as u64);
    assert_eq!(first.pagination.total_pages, Some(6));
    assert!(first.pagination.has_more);
    // 私库优先：bm25 再差也排在第一页最前面
    assert!(
        first.items[..PRIVATE]
            .iter()
            .all(|hit| hit.entry.source == LyricSource::Private)
    );

    let mut seen = HashSet::new();
    for p in 1..=6 {
        let res = search_lyric(&store, &query, page(p, 100)).await;
        assert_eq!(res.pagination.total, TOTAL as u64, "page {p}");
        assert_eq!(res.pagination.has_more, p < 6, "page {p}");
        for hit in res.items {
            assert!(seen.insert(hit.entry.id), "page {p} 与前页重复");
        }
    }
    assert_eq!(seen.len(), TOTAL, "翻完所有页应恰好覆盖全部命中");
}

#[tokio::test]
async fn lyric_search_with_author_id_does_not_miss_low_ranked_hits() {
    let store = crowded_fts_store().await;

    let res = search_lyric(&store, &lyric_query(None, Some("target")), page(1, 50)).await;

    assert_eq!(res.pagination.total, TARGET_PUBLIC as u64);
    let mut names: Vec<&str> = res
        .items
        .iter()
        .map(|hit| hit.entry.filename.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        (0..TARGET_PUBLIC)
            .map(|i| format!("target_{i}.ttml"))
            .collect::<Vec<_>>()
    );

    // 作者与来源同时生效
    let res = search_lyric(
        &store,
        &lyric_query(Some(LyricSource::Private), Some("target")),
        page(1, 50),
    )
    .await;
    assert_eq!(res.pagination.total, 0);
    assert!(res.items.is_empty());
}

/// 本地起一个假的 raw-lyrics 目录服务，代替 GitHub；只认 `remote.ttml`，其余 404
///
/// 返回 (目录 URL, 请求计数)
async fn fake_remote() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    use axum::{
        Router,
        extract::Path,
        http::StatusCode,
        routing::get,
    };

    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let app = Router::new().route(
        "/raw-lyrics/{file}",
        get(move |Path(file): Path<String>| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                if file == "remote.ttml" {
                    Ok(REMOTE_TTML)
                } else {
                    Err(StatusCode::NOT_FOUND)
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await });
    (format!("http://{addr}/raw-lyrics"), hits)
}

const DB_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">from db</p></div></body></tt>"#;
const REMOTE_TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">from remote</p></div></body></tt>"#;

/// 远程层指向本地假服务、SQLite 里有一行 `local.ttml` 的存储
async fn layered_store() -> (DbLyricStore, Arc<std::sync::atomic::AtomicUsize>) {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let parsed = ttml_processor::parse_ttml(DB_TTML).expect("sample ttml");
    let model =
        crate::services::sync_service::build_entity_from_ttml("local.ttml", DB_TTML, &parsed);
    entity::Entity::insert(entity::ActiveModel::from(model))
        .exec(&conn)
        .await
        .expect("seed row");

    let (base, hits) = fake_remote().await;
    let mut store = DbLyricStore::new(conn).with_remote_base(&base);
    // 本机若配了 HTTP(S)_PROXY，环回地址也别走代理
    store.client = Client::builder().no_proxy().build().expect("client");
    (store, hits)
}

async fn delete_all_rows(store: &DbLyricStore) {
    entity::Entity::delete_many()
        .exec(&store.conn)
        .await
        .expect("delete rows");
}

fn remote_hits(hits: &std::sync::atomic::AtomicUsize) -> usize {
    hits.load(std::sync::atomic::Ordering::SeqCst)
}

#[tokio::test]
async fn fetch_lyric_ttml_reads_sqlite_then_serves_from_cache() {
    let (store, hits) = layered_store().await;

    assert_eq!(store.fetch_lyric_ttml("local.ttml").await.unwrap(), DB_TTML);
    assert!(store.ttml_cache.get("local.ttml").await.is_some());

    // 行删掉后仍命中缓存，且整个过程没有碰远程
    delete_all_rows(&store).await;
    assert_eq!(store.fetch_lyric_ttml("local.ttml").await.unwrap(), DB_TTML);
    assert_eq!(remote_hits(&hits), 0);
}

#[tokio::test]
async fn fetch_lyric_ttml_prefers_cache_over_sqlite() {
    let (store, _) = layered_store().await;
    store
        .ttml_cache
        .insert("local.ttml".to_string(), "cached".to_string())
        .await;

    assert_eq!(
        store.fetch_lyric_ttml("local.ttml").await.unwrap(),
        "cached"
    );
}

#[tokio::test]
async fn fetch_lyric_ttml_falls_back_to_remote_once() {
    let (store, hits) = layered_store().await;

    assert_eq!(
        store.fetch_lyric_ttml("remote.ttml").await.unwrap(),
        REMOTE_TTML
    );
    assert_eq!(
        store.fetch_lyric_ttml("remote.ttml").await.unwrap(),
        REMOTE_TTML
    );
    assert_eq!(remote_hits(&hits), 1, "second call is served from cache");
}

#[tokio::test]
async fn fetch_lyric_ttml_remote_miss_is_upstream_error_and_not_cached() {
    let (store, hits) = layered_store().await;

    for _ in 0..2 {
        assert!(matches!(
            store.fetch_lyric_ttml("missing.ttml").await,
            Err(AppError::UpstreamError(_))
        ));
    }
    assert_eq!(remote_hits(&hits), 2, "failures are retried, not cached");
    assert!(store.ttml_cache.get("missing.ttml").await.is_none());
}

#[tokio::test]
async fn fetch_parsed_lyric_caches_formatted_result() {
    let (store, _) = layered_store().await;

    let parsed = store.fetch_parsed_lyric("local.ttml").await.unwrap();
    assert!(
        parsed
            .plain_lyrics
            .as_deref()
            .is_some_and(|t| t.contains("from db"))
    );
    assert!(
        store
            .formatted_lyric_cache
            .get("local.ttml")
            .await
            .is_some()
    );
}

#[tokio::test]
async fn invalidate_caches_clears_both_layers() {
    let (store, hits) = layered_store().await;
    store.fetch_parsed_lyric("local.ttml").await.unwrap();
    store.fetch_parsed_lyric("remote.ttml").await.unwrap();
    assert_eq!(remote_hits(&hits), 1);

    store.invalidate_caches();

    for name in ["local.ttml", "remote.ttml"] {
        assert!(store.ttml_cache.get(name).await.is_none(), "{name}");
        assert!(
            store.formatted_lyric_cache.get(name).await.is_none(),
            "{name}"
        );
    }
    // 失效后重新走 SQLite / 远程
    store.fetch_parsed_lyric("remote.ttml").await.unwrap();
    assert_eq!(remote_hits(&hits), 2);
    delete_all_rows(&store).await;
    assert!(matches!(
        store.fetch_lyric_ttml("local.ttml").await,
        Err(AppError::UpstreamError(_))
    ));
}
