use super::*;
use crate::core::{
    LyricId,
    list_query::{
        IdKind,
        IdKindSet,
    },
    test_utils::make_song,
};

// --- find_by_ids tests ---

#[test]
fn find_by_id_exact_match() {
    let songs = vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &["111"],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &["222"],
            &[],
            &[],
            &[],
        ),
    ];
    let target_id = songs[1].id;
    let db = LyricIndexDB::from_entries(songs);
    let query = IdQuery {
        id: Some(target_id),
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![1]);
}

#[test]
fn find_by_id_not_found() {
    let db = LyricIndexDB::from_entries(vec![make_song(
        "a.ttml",
        100,
        &["Song A"],
        &["Artist X"],
        &["111"],
        &[],
        &[],
        &[],
    )]);
    let query = IdQuery {
        id: Some(LyricId::from_u64(999_999).unwrap()),
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert!(result.is_empty());
}

#[test]
fn find_by_id_takes_priority_over_filename() {
    let songs = vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &[],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &[],
            &[],
            &[],
            &[],
        ),
    ];
    let id_of_b = songs[1].id;
    let db = LyricIndexDB::from_entries(songs);

    let query = IdQuery {
        filename: Some("a.ttml".into()),
        id: Some(id_of_b),
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![1]);
}

#[test]
fn find_by_id_ignores_platform_ids_if_present_in_struct() {
    let songs = vec![make_song(
        "a.ttml",
        100,
        &["Song A"],
        &["Artist X"],
        &["111"],
        &[],
        &[],
        &[],
    )];
    let id = songs[0].id;
    let db = LyricIndexDB::from_entries(songs);
    let query = IdQuery {
        id: Some(id),
        ncm_music_ids: vec!["999".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![0]);
}

#[test]
fn find_by_filename_exact_match() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "1768754400682-250306205-r6IrpmBd.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &["111"],
            &[],
            &[],
            &[],
        ),
        make_song(
            "1768754400683-250306205-r6IrpmBd.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &["222"],
            &[],
            &[],
            &[],
        ),
    ]);
    let query = IdQuery {
        filename: Some("1768754400682-250306205-r6IrpmBd.ttml".into()),
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![0]);
}

#[test]
fn find_by_filename_not_found() {
    let db = LyricIndexDB::from_entries(vec![make_song(
        "1768754400682-250306205-r6IrpmBd.ttml",
        100,
        &["Song A"],
        &["Artist X"],
        &["111"],
        &[],
        &[],
        &[],
    )]);
    let query = IdQuery {
        filename: Some("nonexistent.ttml".into()),
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert!(result.is_empty());
}

#[test]
fn find_by_filename_ignores_other_ids() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &["111"],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &["222"],
            &[],
            &[],
            &[],
        ),
    ]);

    let query = IdQuery {
        filename: Some("b.ttml".into()),
        ncm_music_ids: vec!["111".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![1]);
}

#[test]
fn find_by_single_ncm_id() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &["111"],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &["222"],
            &[],
            &[],
            &[],
        ),
    ]);
    let query = IdQuery {
        ncm_music_ids: vec!["111".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![0]);
}

#[test]
fn find_by_single_spotify_id() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &[],
            &["spot1"],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &[],
            &["spot2"],
            &[],
            &[],
        ),
    ]);
    let query = IdQuery {
        spotify_ids: vec!["spot2".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![1]);
}

#[test]
fn find_by_cross_platform_and() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &["111"],
            &["spot1"],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &["222"],
            &["spot2"],
            &[],
            &[],
        ),
    ]);
    // Entry 0 has ncm=111 & spotify=spot1
    let query = IdQuery {
        ncm_music_ids: vec!["111".into()],
        spotify_ids: vec!["spot1".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![0]);
}

#[test]
fn find_by_cross_platform_and_no_match() {
    let db = LyricIndexDB::from_entries(vec![make_song(
        "a.ttml",
        100,
        &["Song A"],
        &["Artist X"],
        &["111"],
        &["spot1"],
        &[],
        &[],
    )]);
    // Entry 0 has ncm=111 but not spotify=spot2
    let query = IdQuery {
        ncm_music_ids: vec!["111".into()],
        spotify_ids: vec!["spot2".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert!(result.is_empty());
}

#[test]
fn find_by_ids_multiple_same_type_and() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &["111", "222"],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &["222", "333"],
            &[],
            &[],
            &[],
        ),
    ]);
    // AND: need both 111 and 222 -> only entry 0
    let query = IdQuery {
        ncm_music_ids: vec!["111".into(), "222".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![0]);
}

#[test]
fn find_by_ids_no_results() {
    let db = LyricIndexDB::from_entries(vec![make_song(
        "a.ttml",
        100,
        &["Song A"],
        &["Artist X"],
        &["111"],
        &[],
        &[],
        &[],
    )]);
    let query = IdQuery {
        ncm_music_ids: vec!["999".into()],
        ..Default::default()
    };
    let result = db.find_by_ids(&query);
    assert!(result.is_empty());
}

// --- search_by_fields tests ---

#[test]
fn search_by_music_name() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Love Story"],
            &["Taylor Swift"],
            &[],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["ME!"],
            &["Taylor Swift"],
            &[],
            &[],
            &[],
            &[],
        ),
    ]);
    let query = SearchQuery {
        track_name: Some("Love Story".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.filename.as_str(), "a.ttml");
}

#[test]
fn search_by_artist_name() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Taylor Swift"],
            &[],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Ed Sheeran"],
            &[],
            &[],
            &[],
            &[],
        ),
    ]);
    let query = SearchQuery {
        artist_name: Some("Taylor".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.filename.as_str(), "a.ttml");
}

#[test]
fn search_by_fields_filters_source() {
    use crate::core::models::LyricSource;

    let mut public = make_song(
        "public.ttml",
        100,
        &["Same Song"],
        &[],
        &[],
        &[],
        &["111"],
        &[],
    );
    public.source = LyricSource::Public;
    let mut private = make_song(
        "private.ttml",
        200,
        &["Same Song"],
        &[],
        &[],
        &[],
        &["111"],
        &[],
    );
    private.source = LyricSource::Private;
    let db = LyricIndexDB::from_entries(vec![public, private]);

    // 同时覆盖普通候选、作者与文本组合、仅作者精确匹配三个分支。
    for (track_name, author_id) in [
        (Some("Same Song"), None),
        (Some("Same Song"), Some("111")),
        (None, Some("111")),
    ] {
        for (source, filenames) in [
            (Some(LyricSource::Private), vec!["private.ttml"]),
            (Some(LyricSource::Public), vec!["public.ttml"]),
            (None, vec!["private.ttml", "public.ttml"]),
        ] {
            let query = SearchQuery {
                track_name: track_name.map(str::to_owned),
                author_id: author_id.map(str::to_owned),
                source,
                ..Default::default()
            };
            let hits = db.search_by_fields(&query);
            let actual: Vec<_> = hits.iter().map(|hit| hit.entry.filename.as_str()).collect();
            assert_eq!(actual, filenames);
        }
    }
}

#[test]
fn search_by_author_id_exact() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &[],
            &[],
            &["111"],
            &["user1"],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song A"],
            &["Artist X"],
            &[],
            &[],
            &["222"],
            &["user2"],
        ),
    ]);
    let query = SearchQuery {
        author_id: Some("111".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.filename.as_str(), "a.ttml");
}

#[test]
fn search_by_author_id_and_music_name() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Love Story"],
            &["Taylor Swift"],
            &[],
            &[],
            &["111"],
            &["user1"],
        ),
        make_song(
            "b.ttml",
            200,
            &["ME!"],
            &["Taylor Swift"],
            &[],
            &[],
            &["111"],
            &["user1"],
        ),
        make_song(
            "c.ttml",
            300,
            &["Love Story"],
            &["Other Artist"],
            &[],
            &[],
            &["222"],
            &["user2"],
        ),
    ]);
    let query = SearchQuery {
        author_id: Some("111".into()),
        track_name: Some("Love Story".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.filename.as_str(), "a.ttml");
}

#[test]
fn search_by_author_username() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Song A"],
            &["Artist X"],
            &[],
            &[],
            &["111"],
            &["apoint123"],
        ),
        make_song(
            "b.ttml",
            200,
            &["Song B"],
            &["Artist Y"],
            &[],
            &[],
            &["222"],
            &["other"],
        ),
    ]);
    let query = SearchQuery {
        author_username: Some("apoint123".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.filename.as_str(), "a.ttml");
}

#[test]
fn search_by_global_keyword() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "a.ttml",
            100,
            &["Love Story"],
            &["Taylor Swift"],
            &[],
            &[],
            &[],
            &[],
        ),
        make_song(
            "b.ttml",
            200,
            &["Shape of You"],
            &["Ed Sheeran"],
            &[],
            &[],
            &[],
            &[],
        ),
    ]);
    let query = SearchQuery {
        global_keyword: Some("Taylor".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.filename.as_str(), "a.ttml");
}

#[test]
fn search_no_results() {
    let db = LyricIndexDB::from_entries(vec![make_song(
        "a.ttml",
        100,
        &["Love Story"],
        &["Taylor Swift"],
        &[],
        &[],
        &[],
        &[],
    )]);
    let query = SearchQuery {
        track_name: Some("Completely Nonexistent Song Title".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert!(result.is_empty());
}

#[test]
fn search_sorted_by_timestamp_desc_on_equal_score() {
    let db = LyricIndexDB::from_entries(vec![
        make_song(
            "old.ttml",
            100,
            &["ME!"],
            &["Taylor Swift"],
            &[],
            &[],
            &[],
            &[],
        ),
        make_song(
            "new.ttml",
            200,
            &["ME!"],
            &["Taylor Swift"],
            &[],
            &[],
            &[],
            &[],
        ),
    ]);
    let query = SearchQuery {
        track_name: Some("ME!".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);
    assert_eq!(result.len(), 2);
    // Newer first
    assert_eq!(result[0].entry.filename.as_str(), "new.ttml");
    assert_eq!(result[1].entry.filename.as_str(), "old.ttml");
}

#[test]
fn author_only_search_breaks_ties_by_id() {
    // 同一批投稿的 timestamp 可能完全相同，此时必须由 id 兜底给出全序，
    // 否则 sort_unstable_by 的顺序不稳定，翻页会在相邻页之间重复或漏掉条目
    let entries: Vec<_> = ["a.ttml", "b.ttml", "c.ttml", "d.ttml"]
        .into_iter()
        .map(|filename| {
            make_song(
                filename,
                1_768_754_400_682,
                &["ME!"],
                &["Taylor Swift"],
                &[],
                &[],
                &["108002475"],
                &[],
            )
        })
        .collect();
    let db = LyricIndexDB::from_entries(entries);

    // 只按 author 过滤、无任何文本字段，走的是 has_text_fields() 为假的短路分支
    let query = SearchQuery {
        author_id: Some("108002475".into()),
        ..Default::default()
    };
    let result = db.search_by_fields(&query);

    assert_eq!(result.len(), 4);
    let ids: Vec<_> = result.iter().map(|h| h.entry.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "同时间戳条目应按 id 升序给出确定顺序");
}

// --- list_candidates tests ---

/// 标识与作者两个维度都做成不对称的，任何一侧的判定写错都会有测试掉下来
///
/// | 文件 | 携带的标识 | authorId | authorUsername |
/// |---|---|---|---|
/// | `all.ttml` | 全部五种 | `a1` | `alice` |
/// | `ncm.ttml` | 仅 ncm | `a1` | `alice` |
/// | `isrc.ttml` | 仅 isrc | `a2` | `bob` |
/// | `bare.ttml` | 无 | `a2` | `alice` |
fn filter_fixture() -> LyricIndexDB {
    LyricIndexDB::from_entries(vec![
        make_song(
            "all.ttml",
            400,
            &["All"],
            &["Artist"],
            &["111"],
            &["sp1"],
            &["a1"],
            &["alice"],
        )
        .with_qq_music_ids(&["qq1"])
        .with_apple_music_ids(&["ap1"])
        .with_isrcs(&["ISRC1"]),
        make_song(
            "ncm.ttml",
            300,
            &["Ncm"],
            &["Artist"],
            &["222"],
            &[],
            &["a1"],
            &["alice"],
        ),
        make_song(
            "isrc.ttml",
            200,
            &["Isrc"],
            &["Artist"],
            &[],
            &[],
            &["a2"],
            &["bob"],
        )
        .with_isrcs(&["ISRC2"]),
        make_song(
            "bare.ttml",
            100,
            &["Bare"],
            &["Artist"],
            &[],
            &[],
            &["a2"],
            &["alice"],
        ),
    ])
}

fn id_kinds(kinds: &[IdKind]) -> IdKindSet {
    let mut set = IdKindSet::empty();
    for &kind in kinds {
        set.insert(kind);
    }
    set
}

/// `list_candidates` 不保证顺序，断言时统一排序后比文件名
fn matched_names(db: &LyricIndexDB, filter: &ListFilter) -> Vec<String> {
    let mut names: Vec<String> = db
        .list_candidates(filter)
        .into_iter()
        .map(|idx| db.entries[idx].filename.to_string())
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn list_candidates_without_filter_returns_everything() {
    let db = filter_fixture();
    assert_eq!(
        matched_names(&db, &ListFilter::default()),
        ["all.ttml", "bare.ttml", "isrc.ttml", "ncm.ttml"]
    );
}

#[test]
fn list_candidates_filters_by_author_id() {
    let db = filter_fixture();
    let filter = ListFilter {
        author_id: Some("a1".to_string()),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["all.ttml", "ncm.ttml"]);
}

#[test]
fn list_candidates_intersects_both_author_dimensions() {
    let db = filter_fixture();
    let filter = ListFilter {
        author_id: Some("a2".to_string()),
        author_username: Some("alice".to_string()),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["bare.ttml"]);
}

#[test]
fn list_candidates_unknown_author_yields_empty_not_unconstrained() {
    // 查不到的作者必须让整个 AND 为空，而不是让这个条件静默失效
    let db = filter_fixture();
    let filter = ListFilter {
        author_id: Some("nobody".to_string()),
        author_username: Some("alice".to_string()),
        ..ListFilter::default()
    };
    assert!(matched_names(&db, &filter).is_empty());

    let filter = ListFilter {
        author_id: Some("a1".to_string()),
        author_username: Some("nobody".to_string()),
        ..ListFilter::default()
    };
    assert!(matched_names(&db, &filter).is_empty());
}

#[test]
fn list_candidates_requires_every_has_id() {
    let db = filter_fixture();
    let filter = ListFilter {
        has: id_kinds(&[IdKind::NcmMusicId]),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["all.ttml", "ncm.ttml"]);

    // 多个取值之间是 AND，只有同时携带两者的条目命中
    let filter = ListFilter {
        has: id_kinds(&[IdKind::NcmMusicId, IdKind::Isrc]),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["all.ttml"]);
}

#[test]
fn list_candidates_rejects_any_missing_id() {
    let db = filter_fixture();
    let filter = ListFilter {
        missing: id_kinds(&[IdKind::Isrc]),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["bare.ttml", "ncm.ttml"]);

    let filter = ListFilter {
        missing: id_kinds(&[IdKind::Isrc, IdKind::NcmMusicId]),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["bare.ttml"]);
}

#[test]
fn list_candidates_combines_has_and_missing() {
    let db = filter_fixture();
    let filter = ListFilter {
        has: id_kinds(&[IdKind::NcmMusicId]),
        missing: id_kinds(&[IdKind::Isrc]),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["ncm.ttml"]);
}

#[test]
fn list_candidates_combines_author_and_id_kinds() {
    let db = filter_fixture();
    let filter = ListFilter {
        author_username: Some("alice".to_string()),
        missing: id_kinds(&[IdKind::NcmMusicId]),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["bare.ttml"]);
}

#[test]
fn list_candidates_can_match_nothing() {
    // qq ID 只有 all.ttml 有，而它同时带着 ncm ID
    let db = filter_fixture();
    let filter = ListFilter {
        has: id_kinds(&[IdKind::QqMusicId]),
        missing: id_kinds(&[IdKind::NcmMusicId]),
        ..ListFilter::default()
    };
    assert!(matched_names(&db, &filter).is_empty());
}

#[test]
fn list_candidates_filters_by_since() {
    let db = filter_fixture();
    let filter = ListFilter {
        since: Some(300),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["all.ttml", "ncm.ttml"]);
}

#[test]
fn list_candidates_filters_by_until() {
    let db = filter_fixture();
    let filter = ListFilter {
        until: Some(200),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["bare.ttml", "isrc.ttml"]);
}

#[test]
fn list_candidates_filters_by_since_and_until_window() {
    let db = filter_fixture();
    let filter = ListFilter {
        since: Some(200),
        until: Some(300),
        ..ListFilter::default()
    };
    assert_eq!(matched_names(&db, &filter), ["isrc.ttml", "ncm.ttml"]);
}

#[test]
fn find_by_ids_filename_hits_private_first() {
    use crate::core::models::LyricSource;
    let mut priv_entry = make_song("priv.ttml", 1, &["T"], &["A"], &[], &[], &[], &[]);
    priv_entry.source = LyricSource::Private;
    priv_entry.id = LyricId::from_private_filename("priv.ttml");
    let pub_entry = make_song("pub.ttml", 2, &["T"], &["A"], &[], &[], &[], &[]);
    let db = LyricIndexDB::from_entries(vec![priv_entry, pub_entry]);
    let query = IdQuery {
        filename: Some("priv.ttml".to_string()),
        ..IdQuery::default()
    };
    let result = db.find_by_ids(&query);
    assert_eq!(result, vec![0]);
}

// --- search_by_fields source 过滤 ---

fn source_fixture() -> LyricIndexDB {
    use crate::core::models::LyricSource;
    let pub_entry = make_song(
        "pub.ttml",
        1,
        &["Same Song"],
        &["A"],
        &[],
        &[],
        &["u1"],
        &[],
    );
    let mut priv_entry = make_song(
        "priv.ttml",
        2,
        &["Same Song"],
        &["A"],
        &[],
        &[],
        &["u1"],
        &[],
    );
    priv_entry.source = LyricSource::Private;
    priv_entry.id = LyricId::from_private_filename("priv.ttml");
    LyricIndexDB::from_entries(vec![pub_entry, priv_entry])
}

fn searched_names(db: &LyricIndexDB, query: &SearchQuery) -> Vec<String> {
    let mut names: Vec<String> = db
        .search_by_fields(query)
        .iter()
        .map(|hit| hit.entry.filename.to_string())
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn search_by_fields_filters_by_source() {
    use crate::core::models::LyricSource;
    let db = source_fixture();
    let query = |source| SearchQuery {
        track_name: Some("Same Song".to_string()),
        source,
        ..Default::default()
    };

    assert_eq!(
        searched_names(&db, &query(Some(LyricSource::Private))),
        ["priv.ttml"]
    );
    assert_eq!(
        searched_names(&db, &query(Some(LyricSource::Public))),
        ["pub.ttml"]
    );
    assert_eq!(searched_names(&db, &query(None)), ["priv.ttml", "pub.ttml"]);
}

#[test]
fn search_by_fields_author_branch_filters_by_source() {
    use crate::core::models::LyricSource;
    let db = source_fixture();
    let query = |source| SearchQuery {
        author_id: Some("u1".to_string()),
        source,
        ..Default::default()
    };

    assert_eq!(
        searched_names(&db, &query(Some(LyricSource::Private))),
        ["priv.ttml"]
    );
    assert_eq!(
        searched_names(&db, &query(Some(LyricSource::Public))),
        ["pub.ttml"]
    );
    assert_eq!(searched_names(&db, &query(None)), ["priv.ttml", "pub.ttml"]);
}
