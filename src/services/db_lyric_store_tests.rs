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
