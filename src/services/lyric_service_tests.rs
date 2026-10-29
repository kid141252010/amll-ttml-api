use std::{
    collections::HashMap,
    sync::Arc,
};

use super::*;
use crate::{
    core::{
        list_query::{
            IdKind,
            IdKindSet,
            ListFilter,
        },
        models::{
            LyricIndexDB,
            LyricMatchField,
            LyricSource,
        },
        test_utils::make_song,
    },
    utils::ttml::parse_and_format_ttml,
};

#[derive(Default)]
pub struct MemoryLyricStore {
    pub db: Arc<LyricIndexDB>,
    pub ttml_map: HashMap<String, String>,
    pub fts_results: HashMap<String, Vec<LyricHit>>,
}

#[allow(clippy::unused_async_trait_impl)]
impl LyricStore for MemoryLyricStore {
    async fn fetch_lyric_ttml(&self, filename: &str) -> Result<String, AppError> {
        self.ttml_map
            .get(filename)
            .cloned()
            .ok_or(AppError::LyricNotFound)
    }

    async fn fetch_parsed_lyric(&self, filename: &str) -> Result<TTMLFormatResult, AppError> {
        let ttml = self.fetch_lyric_ttml(filename).await?;
        Ok(parse_and_format_ttml(&ttml))
    }

    async fn search_lyrics_fts(
        &self,
        keyword: &str,
        _limit: u64,
    ) -> Result<Vec<LyricHit>, AppError> {
        Ok(self.fts_results.get(keyword).cloned().unwrap_or_default())
    }

    async fn load_index(&self) -> Arc<LyricIndexDB> {
        Arc::clone(&self.db)
    }
}

fn sample_ttml() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?>
<tt xmlns="http://www.w3.org/ns/ttml">
  <body>
    <div>
      <p begin="00:01.000" end="00:03.000">Hello World Lyric</p>
    </div>
  </body>
</tt>"#
        .to_string()
}

fn create_test_store() -> (MemoryLyricStore, LyricId, LyricId) {
    let entry1 = make_song(
        "test_song_one.ttml",
        1_600_000_000,
        &["Test Song One"],
        &["Artist Alpha"],
        &["1001"],
        &["sp1001"],
        &[],
        &[],
    );
    let id1 = entry1.id;

    let entry2 = make_song(
        "test_song_two.ttml",
        1_700_000_000,
        &["Test Song Two"],
        &["Artist Beta"],
        &["1002"],
        &["sp1002"],
        &[],
        &[],
    );
    let id2 = entry2.id;

    let db = LyricIndexDB::from_entries(vec![entry1, entry2]);
    let mut ttml_map = HashMap::new();
    ttml_map.insert("test_song_one.ttml".to_string(), sample_ttml());
    ttml_map.insert("test_song_two.ttml".to_string(), sample_ttml());

    let mut fts_results = HashMap::new();
    fts_results.insert(
        "Hello".to_string(),
        vec![LyricHit {
            id: id1,
            rank: 0.1,
            field: LyricMatchField::MainLyric,
            snippet: Some("Hello World Lyric".to_string()),
        }],
    );

    (
        MemoryLyricStore {
            db: Arc::new(db),
            ttml_map,
            fts_results,
        },
        id1,
        id2,
    )
}

fn default_pagination() -> Pagination {
    Pagination {
        page: 1,
        page_size: 10,
    }
}

fn default_list_query() -> ListQuery {
    ListQuery {
        pagination: default_pagination(),
        ..ListQuery::default()
    }
}

#[tokio::test]
async fn list_lyrics_orders_newest_first() {
    let (store, id1, id2) = create_test_store();

    let res = list_lyrics(&store, default_list_query()).await;

    assert_eq!(res.items.len(), 2);
    // test_song_two 的 timestamp 更大，应排在前
    assert_eq!(res.items[0].id, id2);
    assert_eq!(res.items[1].id, id1);
    assert_eq!(res.pagination.total, 2);
    assert_eq!(res.pagination.total_pages, Some(1));
    assert!(!res.pagination.has_more);
}

#[tokio::test]
async fn list_lyrics_private_first_and_source_filter() {
    // 私库条目时间戳更旧，但默认排序下必须排在公库前面
    let mut private_entry = make_song("mine.ttml", 1_000, &["Mine"], &["Me"], &[], &[], &[], &[]);
    private_entry.source = LyricSource::Private;
    let public_entry = make_song(
        "theirs.ttml",
        2_000,
        &["Theirs"],
        &["Them"],
        &[],
        &[],
        &[],
        &[],
    );
    let store = MemoryLyricStore {
        db: Arc::new(LyricIndexDB::from_entries(vec![
            public_entry,
            private_entry,
        ])),
        ..MemoryLyricStore::default()
    };

    let res = list_lyrics(&store, default_list_query()).await;
    assert_eq!(
        res.items
            .iter()
            .map(|e| e.filename.as_str())
            .collect::<Vec<_>>(),
        ["mine.ttml", "theirs.ttml"]
    );

    let only_private = list_lyrics(
        &store,
        ListQuery {
            filter: ListFilter {
                source: Some(LyricSource::Private),
                ..ListFilter::default()
            },
            ..default_list_query()
        },
    )
    .await;
    assert_eq!(only_private.pagination.total, 1);
    assert_eq!(only_private.items[0].filename.as_str(), "mine.ttml");

    let only_public = list_lyrics(
        &store,
        ListQuery {
            filter: ListFilter {
                source: Some(LyricSource::Public),
                ..ListFilter::default()
            },
            ..default_list_query()
        },
    )
    .await;
    assert_eq!(only_public.pagination.total, 1);
    assert_eq!(only_public.items[0].filename.as_str(), "theirs.ttml");
}

#[tokio::test]
async fn list_lyrics_paginates() {
    let (store, id1, id2) = create_test_store();
    let one_per_page = Pagination {
        page: 1,
        page_size: 1,
    };

    let first = list_lyrics(
        &store,
        ListQuery {
            pagination: one_per_page,
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].id, id2);
    assert_eq!(first.pagination.total, 2);
    assert_eq!(first.pagination.total_pages, Some(2));
    assert!(first.pagination.has_more);

    let second = list_lyrics(
        &store,
        ListQuery {
            pagination: Pagination {
                page: 2,
                page_size: 1,
            },
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].id, id1);
    assert!(!second.pagination.has_more);
}

#[tokio::test]
async fn list_lyrics_page_past_end_is_empty_but_reports_total() {
    let (store, _, _) = create_test_store();

    let res = list_lyrics(
        &store,
        ListQuery {
            pagination: Pagination {
                page: 99,
                page_size: 10,
            },
            ..ListQuery::default()
        },
    )
    .await;

    assert!(res.items.is_empty());
    assert_eq!(res.pagination.total, 2);
    assert!(!res.pagination.has_more);
}

#[tokio::test]
async fn list_lyrics_breaks_ties_by_id() {
    // 同一批投稿的 timestamp 可能完全相同，此时必须由 id 兜底给出确定的全序，
    // 否则翻页会在相邻页之间重复或漏掉条目
    let entries: Vec<_> = ["a.ttml", "b.ttml", "c.ttml"]
        .into_iter()
        .map(|filename| {
            make_song(
                filename,
                1_768_754_400_682,
                &["Same Timestamp"],
                &["Artist"],
                &[],
                &[],
                &[],
                &[],
            )
        })
        .collect();
    let store = MemoryLyricStore {
        db: Arc::new(LyricIndexDB::from_entries(entries)),
        ..Default::default()
    };

    let res = list_lyrics(&store, default_list_query()).await;

    assert_eq!(res.items.len(), 3);
    let ids: Vec<_> = res.items.iter().map(|entry| entry.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "同时间戳条目应按 id 升序给出确定顺序");
}

#[tokio::test]
async fn list_lyrics_supports_artist_and_album_sort() {
    let (mut store, id1, id2) = create_test_store();

    store.db = Arc::new(LyricIndexDB::from_entries(
        store
            .db
            .entries
            .iter()
            .cloned()
            .map(|mut entry| {
                if entry.id == id1 {
                    entry.artist_names = ["Zed Artist".into()].into();
                    entry.album_names = ["Alpha Album".into()].into();
                } else {
                    entry.artist_names = ["Amy Artist".into()].into();
                    entry.album_names = ["Zulu Album".into()].into();
                }
                entry
            })
            .collect(),
    ));

    let artist_name_asc = list_lyrics(
        &store,
        ListQuery {
            sort: ListSort::ArtistName,
            order: ListOrder::Asc,
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(artist_name_asc.items[0].id, id2);

    let album_name_desc = list_lyrics(
        &store,
        ListQuery {
            sort: ListSort::AlbumName,
            order: ListOrder::Desc,
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(album_name_desc.items[0].id, id2);
}

#[tokio::test]
async fn list_lyrics_supports_sort_and_order() {
    let (store, id1, id2) = create_test_store();

    let track_name_asc = list_lyrics(
        &store,
        ListQuery {
            sort: ListSort::TrackName,
            order: ListOrder::Asc,
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(
        track_name_asc
            .items
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![id1, id2]
    );

    let track_name_desc = list_lyrics(
        &store,
        ListQuery {
            sort: ListSort::TrackName,
            order: ListOrder::Desc,
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(
        track_name_desc
            .items
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![id2, id1]
    );

    let id_desc = list_lyrics(
        &store,
        ListQuery {
            sort: ListSort::Id,
            order: ListOrder::Desc,
            ..ListQuery::default()
        },
    )
    .await;
    let ids: Vec<_> = id_desc.items.iter().map(|entry| entry.id).collect();
    let mut expected = ids.clone();
    expected.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(ids, expected);

    let created_at_asc = list_lyrics(
        &store,
        ListQuery {
            sort: ListSort::CreatedAt,
            order: ListOrder::Asc,
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(
        created_at_asc
            .items
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![id1, id2]
    );
}

/// 三条歌词：`a1` / `alice` 有两条（其一带 ISRC），`a2` / `bob` 有一条（无 ncm 也无 ISRC）
fn filter_test_store() -> MemoryLyricStore {
    let entries = vec![
        make_song(
            "with_isrc.ttml",
            300,
            &["Filter A"],
            &["Artist"],
            &["1001"],
            &[],
            &["a1"],
            &["alice"],
        )
        .with_isrcs(&["ISRC1"]),
        make_song(
            "no_isrc.ttml",
            200,
            &["Filter B"],
            &["Artist"],
            &["1002"],
            &[],
            &["a1"],
            &["alice"],
        ),
        make_song(
            "other_author.ttml",
            100,
            &["Filter C"],
            &["Artist"],
            &[],
            &[],
            &["a2"],
            &["bob"],
        ),
    ];

    MemoryLyricStore {
        db: Arc::new(LyricIndexDB::from_entries(entries)),
        ..Default::default()
    }
}

fn filtered_filenames(res: &Paginated<SongEntry>) -> Vec<&str> {
    res.items
        .iter()
        .map(|entry| entry.filename.as_str())
        .collect()
}

#[tokio::test]
async fn list_lyrics_filter_shrinks_total() {
    let store = filter_test_store();

    let mut has_isrc = IdKindSet::empty();
    has_isrc.insert(IdKind::Isrc);

    let res = list_lyrics(
        &store,
        ListQuery {
            filter: ListFilter {
                has: has_isrc,
                ..ListFilter::default()
            },
            ..default_list_query()
        },
    )
    .await;

    assert_eq!(filtered_filenames(&res), ["with_isrc.ttml"]);
    // total 是过滤后的数量，不是索引里的条目总数
    assert_eq!(res.pagination.total, 1);
    assert_eq!(res.pagination.total_pages, Some(1));
    assert!(!res.pagination.has_more);
}

#[tokio::test]
async fn list_lyrics_filter_respects_sort_and_order() {
    let store = filter_test_store();
    let filter = ListFilter {
        author_username: Some("alice".to_string()),
        ..ListFilter::default()
    };

    let desc = list_lyrics(
        &store,
        ListQuery {
            filter: filter.clone(),
            ..default_list_query()
        },
    )
    .await;
    assert_eq!(
        filtered_filenames(&desc),
        ["with_isrc.ttml", "no_isrc.ttml"]
    );

    let asc = list_lyrics(
        &store,
        ListQuery {
            order: ListOrder::Asc,
            filter,
            ..default_list_query()
        },
    )
    .await;
    assert_eq!(filtered_filenames(&asc), ["no_isrc.ttml", "with_isrc.ttml"]);
}

#[tokio::test]
async fn list_lyrics_filter_without_matches_is_empty_not_an_error() {
    let store = filter_test_store();

    let mut has_ncm = IdKindSet::empty();
    has_ncm.insert(IdKind::NcmMusicId);

    let res = list_lyrics(
        &store,
        ListQuery {
            filter: ListFilter {
                author_id: Some("a2".to_string()),
                has: has_ncm,
                ..ListFilter::default()
            },
            ..default_list_query()
        },
    )
    .await;

    assert!(res.items.is_empty());
    assert_eq!(res.pagination.total, 0);
    assert_eq!(res.pagination.total_pages, Some(0));
    assert!(!res.pagination.has_more);
}

#[tokio::test]
async fn list_lyrics_cursor_walks_across_pages() {
    use crate::core::list_query::Cursor;

    let (store, id1, id2) = create_test_store();

    // 第 1 页：offset 模式 pageSize=1，应带回 id2 与 nextCursor
    let first = list_lyrics(
        &store,
        ListQuery {
            pagination: Pagination {
                page: 1,
                page_size: 1,
            },
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].id, id2);
    assert!(first.pagination.has_more);
    let cursor_str = first.pagination.next_cursor.expect("应当携带 next_cursor");

    // 第 2 页：游标模式传入第一页末尾游标，应带回 id1，且无更多数据
    let second = list_lyrics(
        &store,
        ListQuery {
            pagination: Pagination {
                page: 1,
                page_size: 1,
            },
            cursor: Some(Cursor::parse(&cursor_str).unwrap()),
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].id, id1);
    assert_eq!(second.pagination.page, None);
    assert_eq!(second.pagination.total_pages, None);
    assert_eq!(second.pagination.total, 2);
    assert!(!second.pagination.has_more);
    assert!(second.pagination.next_cursor.is_none());

    // 第 3 页：使用第 2 页末尾的标识再次查询，应为空页
    let third_cursor = format!("{}_{}", second.items[0].timestamp, id1.get());
    let third = list_lyrics(
        &store,
        ListQuery {
            pagination: Pagination {
                page: 1,
                page_size: 1,
            },
            cursor: Some(Cursor::parse(&third_cursor).unwrap()),
            ..ListQuery::default()
        },
    )
    .await;
    assert!(third.items.is_empty());
    assert!(!third.pagination.has_more);
}

#[tokio::test]
async fn list_lyrics_cursor_supports_asc_order() {
    use crate::core::list_query::Cursor;

    let (store, id1, id2) = create_test_store();

    // Asc 排序下，id1（较旧）在前，id2（较新）在后
    let first = list_lyrics(
        &store,
        ListQuery {
            pagination: Pagination {
                page: 1,
                page_size: 1,
            },
            order: ListOrder::Asc,
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(first.items[0].id, id1);
    assert!(first.pagination.has_more);
    let cursor_str = first.pagination.next_cursor.unwrap();

    let second = list_lyrics(
        &store,
        ListQuery {
            pagination: Pagination {
                page: 1,
                page_size: 1,
            },
            order: ListOrder::Asc,
            cursor: Some(Cursor::parse(&cursor_str).unwrap()),
            ..ListQuery::default()
        },
    )
    .await;
    assert_eq!(second.items[0].id, id2);
    assert!(!second.pagination.has_more);
}

#[tokio::test]
async fn list_lyrics_since_and_until_filters() {
    let store = filter_test_store();

    // 筛选 since=200：命中 300(with_isrc) 和 200(no_isrc)
    let res_since = list_lyrics(
        &store,
        ListQuery {
            filter: ListFilter {
                since: Some(200),
                ..ListFilter::default()
            },
            ..default_list_query()
        },
    )
    .await;
    assert_eq!(
        filtered_filenames(&res_since),
        ["with_isrc.ttml", "no_isrc.ttml"]
    );
    assert_eq!(res_since.pagination.total, 2);

    // 筛选 until=200：命中 200(no_isrc) 和 100(other_author)
    let res_until = list_lyrics(
        &store,
        ListQuery {
            filter: ListFilter {
                until: Some(200),
                ..ListFilter::default()
            },
            ..default_list_query()
        },
    )
    .await;
    assert_eq!(
        filtered_filenames(&res_until),
        ["no_isrc.ttml", "other_author.ttml"]
    );
    assert_eq!(res_until.pagination.total, 2);

    // 筛选 since=200, until=200 时间窗口：仅命中 200(no_isrc)
    let res_window = list_lyrics(
        &store,
        ListQuery {
            filter: ListFilter {
                since: Some(200),
                until: Some(200),
                ..ListFilter::default()
            },
            ..default_list_query()
        },
    )
    .await;
    assert_eq!(filtered_filenames(&res_window), ["no_isrc.ttml"]);
    assert_eq!(res_window.pagination.total, 1);
}

#[tokio::test]
async fn search_lyric_metadata_hit() {
    let (store, id1, _) = create_test_store();
    let query = SearchQuery {
        track_name: Some("Test Song One".to_string()),
        ..Default::default()
    };

    let res = search_lyric(&store, &query, default_pagination()).await;
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].entry.id, id1);
    assert_eq!(res.items[0].entry.filename.as_str(), "test_song_one.ttml");
    assert!(res.items[0].lyric_hit.is_none());
    assert_eq!(res.pagination.total, 1);
    assert!(!res.pagination.has_more);
}

#[tokio::test]
async fn search_lyric_filters_fts_hits_by_source() {
    let (mut store, public_id, private_id) = create_test_store();
    let mut entries = store.db.entries.clone();
    entries
        .iter_mut()
        .find(|entry| entry.id == private_id)
        .unwrap()
        .source = LyricSource::Private;
    store.db = Arc::new(LyricIndexDB::from_entries(entries));
    store.fts_results.get_mut("Hello").unwrap().push(LyricHit {
        id: private_id,
        rank: 0.2,
        field: LyricMatchField::MainLyric,
        snippet: Some("Hello private lyric".to_string()),
    });

    for (source, expected_ids) in [
        (Some(LyricSource::Private), vec![private_id]),
        (Some(LyricSource::Public), vec![public_id]),
        (None, vec![public_id, private_id]),
    ] {
        let query = SearchQuery {
            lyric_text: Some("Hello".to_string()),
            source,
            ..Default::default()
        };
        let res = search_lyric(&store, &query, default_pagination()).await;
        assert_eq!(res.items.len(), expected_ids.len());
        for id in expected_ids {
            assert!(res.items.iter().any(|hit| hit.entry.id == id));
        }
    }
}

#[tokio::test]
async fn search_lyric_fts_hit_carries_lyric_hit() {
    let (store, id1, _) = create_test_store();
    let query = SearchQuery {
        lyric_text: Some("Hello".to_string()),
        ..Default::default()
    };

    let res = search_lyric(&store, &query, default_pagination()).await;
    assert_eq!(res.items.len(), 1);
    assert_eq!(res.items[0].entry.id, id1);

    let lyric_hit = res.items[0].lyric_hit.as_ref().unwrap();
    assert_eq!(lyric_hit.field, LyricMatchField::MainLyric);
    assert_eq!(lyric_hit.snippet.as_deref(), Some("Hello World Lyric"));
}

#[tokio::test]
async fn get_lyric_returns_entry_and_ttml() {
    let (store, id1, _) = create_test_store();
    let query = IdQuery {
        spotify_ids: vec!["sp1001".to_string()],
        ..Default::default()
    };

    let (entry, ttml_text) = get_lyric(&store, query).await.unwrap();
    assert_eq!(entry.id, id1);
    assert_eq!(ttml_text, sample_ttml());

    let invalid_query = IdQuery {
        spotify_ids: vec!["non_existent".to_string()],
        ..Default::default()
    };
    let err = get_lyric(&store, invalid_query).await;
    assert!(matches!(err, Err(AppError::LyricNotFound)));
}

#[tokio::test]
async fn lrclib_search_returns_entries_with_parsed_lyrics() {
    let (store, _, _) = create_test_store();
    let query = SearchQuery {
        global_keyword: Some("Artist".to_string()),
        ..Default::default()
    };

    let res = lrclib_search(&store, &query, default_pagination()).await;
    assert_eq!(res.items.len(), 2);
    assert_eq!(res.pagination.total, 2);
    for (entry, formatted) in &res.items {
        assert!(!entry.filename.as_str().is_empty());
        let formatted = formatted.as_ref().unwrap();
        assert_eq!(formatted.plain_lyrics.as_deref(), Some("Hello World Lyric"));
        assert_eq!(
            formatted.synced_lyrics.as_deref(),
            Some("[00:01.00] Hello World Lyric")
        );
    }
}

#[tokio::test]
async fn lrclib_get_by_fields_returns_entry_and_parsed_lyric() {
    let (store, id1, _) = create_test_store();
    let query = SearchQuery {
        track_name: Some("Test Song One".to_string()),
        artist_name: Some("Artist Alpha".to_string()),
        ..Default::default()
    };

    let (entry, formatted) = lrclib_get_by_fields(&store, &query).await.unwrap();
    assert_eq!(entry.id, id1);
    assert_eq!(entry.track_names[0].as_str(), "Test Song One");
    assert_eq!(formatted.plain_lyrics.as_deref(), Some("Hello World Lyric"));
    assert_eq!(
        formatted.synced_lyrics.as_deref(),
        Some("[00:01.00] Hello World Lyric")
    );

    let no_match_query = SearchQuery {
        track_name: Some("NonExistentTrack".to_string()),
        ..Default::default()
    };
    let err = lrclib_get_by_fields(&store, &no_match_query).await;
    assert!(matches!(err, Err(AppError::LyricNotFound)));
}

#[tokio::test]
async fn lrclib_get_by_id_returns_entry_and_parsed_lyric() {
    let (store, _, id2) = create_test_store();
    let (entry, formatted) = lrclib_get_by_id(&store, id2).await.unwrap();
    assert_eq!(entry.id, id2);
    assert!(formatted.synced_lyrics.is_some());

    let err = lrclib_get_by_id(&store, LyricId::from_u64(9999).unwrap()).await;
    assert!(matches!(err, Err(AppError::LyricNotFound)));
}
