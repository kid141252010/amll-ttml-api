use super::*;

// --- Native Search 测试组 ---

#[test]
fn native_search_no_params_returns_error() {
    let result = parse_search_query("", &NATIVE_SEARCH_DIALECT);
    assert!(result.is_err());
    if let Err(AppError::BadRequest(msg)) = result {
        assert_eq!(msg, "Missing valid search parameters.");
    }
}

#[test]
fn native_search_empty_string_params_returns_error() {
    let result = parse_search_query("musicName=&artistName=", &NATIVE_SEARCH_DIALECT);
    assert!(result.is_err());
}

#[test]
fn search_source_parsing() {
    for (raw, expected) in [
        ("private", Some(LyricSource::Private)),
        ("public", Some(LyricSource::Public)),
        ("+private+", Some(LyricSource::Private)),
        ("all", None),
    ] {
        let (query, _) =
            parse_search_query(&format!("q=x&source={raw}"), &NATIVE_SEARCH_DIALECT).unwrap();
        assert_eq!(query.source, expected);
        assert_eq!(query.global_keyword.as_deref(), Some("x"));

        let exact = parse_search_query_exact(
            &format!("track_name=x&artist_name=y&source={raw}"),
            &LRCLIB_GET_DIALECT,
        )
        .unwrap();
        assert_eq!(exact.source, expected);
    }
}

/// `source` 与 `sort` / `order` / `hasId` 一样区分大小写
#[test]
fn search_source_is_case_sensitive() {
    for raw in ["Private", "PUBLIC", "All"] {
        let message = format!("'source' must be one of 'private', 'public', 'all', got '{raw}'.");
        assert!(matches!(
            parse_search_query(&format!("q=x&source={raw}"), &NATIVE_SEARCH_DIALECT),
            Err(AppError::BadRequest(ref msg)) if *msg == message
        ));
        assert!(matches!(
            parse_search_query_exact(
                &format!("track_name=x&artist_name=y&source={raw}"),
                &LRCLIB_GET_DIALECT,
            ),
            Err(AppError::BadRequest(ref msg)) if *msg == message
        ));
        assert!(matches!(
            parse_list_query(&format!("source={raw}")),
            Err(AppError::BadRequest(ref msg)) if *msg == message
        ));
    }
}

#[test]
fn search_invalid_source_returns_bad_request() {
    let results = [
        parse_search_query("q=x&source=bogus", &NATIVE_SEARCH_DIALECT).map(|(query, _)| query),
        parse_search_query_exact(
            "track_name=x&artist_name=y&source=bogus",
            &LRCLIB_GET_DIALECT,
        ),
    ];
    for result in results {
        assert!(matches!(result, Err(AppError::BadRequest(msg)) if msg ==
            "'source' must be one of 'private', 'public', 'all', got 'bogus'."));
    }
}

#[test]
fn search_source_only_still_requires_keywords() {
    assert!(matches!(
        parse_search_query("source=private", &NATIVE_SEARCH_DIALECT),
        Err(AppError::BadRequest(msg)) if msg == "Missing valid search parameters."
    ));
}

#[test]
fn native_search_q_only() {
    let (query, pagination) = parse_search_query("q=Taylor+Swift", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("Taylor Swift"));
    assert!(query.track_name.is_none());
    assert_eq!(pagination.page, 1);
    assert_eq!(pagination.page_size, 50);
}

#[test]
fn native_search_music_name_only() {
    let (query, _) = parse_search_query("musicName=ME!", &NATIVE_SEARCH_DIALECT).unwrap();
    assert!(query.global_keyword.is_none());
    assert_eq!(query.track_name.as_deref(), Some("ME!"));
}

#[test]
fn native_search_q_with_non_q_ignores_q() {
    let (query, _) =
        parse_search_query("q=Taylor+Swift&musicName=ME!", &NATIVE_SEARCH_DIALECT).unwrap();
    assert!(query.global_keyword.is_none());
    assert_eq!(query.track_name.as_deref(), Some("ME!"));
}

#[test]
fn native_search_multiple_non_q_params_preserved() {
    let (query, _) = parse_search_query(
        "musicName=ME!&artistName=Taylor+Swift&authorId=108002475",
        &NATIVE_SEARCH_DIALECT,
    )
    .unwrap();
    assert_eq!(query.track_name.as_deref(), Some("ME!"));
    assert_eq!(query.artist_name.as_deref(), Some("Taylor Swift"));
    assert_eq!(query.author_id.as_deref(), Some("108002475"));
}

#[test]
fn native_search_unknown_params_ignored() {
    let (query, _) = parse_search_query("q=hello&unknown=value", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("hello"));
}

#[test]
fn native_search_empty_value_skipped() {
    let (query, _) = parse_search_query("musicName=&q=hello", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("hello"));
    assert!(query.track_name.is_none());
}

#[test]
fn native_search_source_param() {
    let (query, _) = parse_search_query("q=x&source=private", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.source, Some(LyricSource::Private));

    let (query, _) = parse_search_query("q=x&source=public", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.source, Some(LyricSource::Public));

    // all 与未传等价
    let (query, _) = parse_search_query("q=x&source=all", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.source, None);
    let (query, _) = parse_search_query("q=x", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.source, None);
}

#[test]
fn native_search_invalid_source_returns_error() {
    let result = parse_search_query("q=x&source=bogus", &NATIVE_SEARCH_DIALECT);
    assert!(matches!(
        result,
        Err(AppError::BadRequest(ref msg))
            if msg == "'source' must be one of 'private', 'public', 'all', got 'bogus'."
    ));
}

#[test]
fn native_search_source_alone_is_not_a_search_param() {
    let result = parse_search_query("source=private", &NATIVE_SEARCH_DIALECT);
    assert!(matches!(
        result,
        Err(AppError::BadRequest(ref msg)) if msg == "Missing valid search parameters."
    ));
}

/// list 专有参数在 search 上是 400，不静默忽略
#[test]
fn native_search_rejects_list_only_params() {
    for param in LIST_ONLY_PARAMS {
        let result = parse_search_query(&format!("q=x&{param}=1"), &NATIVE_SEARCH_DIALECT);
        let expected = format!("'{param}' is only supported by '/lyrics/list'.");
        assert!(
            matches!(result, Err(AppError::BadRequest(ref msg)) if *msg == expected),
            "{param} should be rejected"
        );
    }

    // 先于「缺检索参数」报错，给出更具体的原因
    assert!(matches!(
        parse_search_query("cursor=1768754400682_699269132670751", &NATIVE_SEARCH_DIALECT),
        Err(AppError::BadRequest(ref msg)) if msg == "'cursor' is only supported by '/lyrics/list'."
    ));
}

/// search 真正支持的参数（含与 list 同名的 page / pageSize / source / authorId / authorUsername）
/// 不受 list 专有参数拦截的影响；空值与其他参数一样跳过
#[test]
fn native_search_list_only_guard_keeps_supported_params() {
    let (query, pagination) = parse_search_query(
        "q=x&page=2&pageSize=10&source=private&authorId=1&authorUsername=u&cursor=&sort=",
        &NATIVE_SEARCH_DIALECT,
    )
    .unwrap();
    assert_eq!(pagination.page, 2);
    assert_eq!(pagination.page_size, 10);
    assert_eq!(query.source, Some(LyricSource::Private));
    assert_eq!(query.author_id.as_deref(), Some("1"));
    assert_eq!(query.author_username.as_deref(), Some("u"));
}

/// lrclib 兼容端点沿用原版行为，未知参数（含 list 专有参数名）一律忽略
#[test]
fn lrclib_dialects_ignore_list_only_params() {
    let (query, _) =
        parse_search_query("q=x&cursor=1&sort=id&since=1", &LRCLIB_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("x"));

    let query = parse_search_query_exact(
        "track_name=x&artist_name=y&cursor=1&order=asc",
        &LRCLIB_GET_DIALECT,
    )
    .unwrap();
    assert_eq!(query.track_name.as_deref(), Some("x"));
}

#[test]
fn native_search_whitespace_only_value_skipped() {
    let (query, _) = parse_search_query("musicName=+&q=hello", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("hello"));
    assert!(query.track_name.is_none());
}

#[test]
fn native_search_all_params_populated() {
    let (query, _) = parse_search_query(
        "q=ignored&musicName=a&artistName=b&albumName=c&authorId=d&authorUsername=e",
        &NATIVE_SEARCH_DIALECT,
    )
    .unwrap();
    assert!(query.global_keyword.is_none());
    assert_eq!(query.track_name.as_deref(), Some("a"));
    assert_eq!(query.artist_name.as_deref(), Some("b"));
    assert_eq!(query.album_name.as_deref(), Some("c"));
    assert_eq!(query.author_id.as_deref(), Some("d"));
    assert_eq!(query.author_username.as_deref(), Some("e"));
}

#[test]
fn native_search_page_and_page_size_parsed() {
    let (query, pagination) =
        parse_search_query("q=hello&page=3&pageSize=20", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("hello"));
    assert_eq!(pagination.page, 3);
    assert_eq!(pagination.page_size, 20);
}

#[test]
fn native_search_page_params_do_not_set_has_non_q_flag() {
    let result = parse_search_query("page=2&pageSize=10", &NATIVE_SEARCH_DIALECT);
    assert!(result.is_err());
}

#[test]
fn native_search_page_params_do_not_trigger_q_discard() {
    let (query, pagination) = parse_search_query("q=love&page=2", &NATIVE_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("love"));
    assert_eq!(pagination.page, 2);
}

#[test]
fn native_search_invalid_page_returns_error() {
    let result = parse_search_query("q=hello&page=0", &NATIVE_SEARCH_DIALECT);
    assert!(result.is_err());
}

#[test]
fn native_search_invalid_page_size_returns_error() {
    let result = parse_search_query("q=hello&pageSize=200", &NATIVE_SEARCH_DIALECT);
    assert!(result.is_err());
}

// --- 仅分页（列表端点）测试组 ---

#[test]
fn pagination_only_no_params_uses_defaults() {
    // 与 parse_search_query 的关键差异：列表端点不带查询串也是合法请求
    let pagination = parse_list_query("").unwrap().pagination;
    assert_eq!(pagination, Pagination::default());
    assert_eq!(pagination.page, 1);
    assert_eq!(pagination.page_size, 50);
}

#[test]
fn pagination_only_page_and_page_size_parsed() {
    let pagination = parse_list_query("page=2&pageSize=10").unwrap().pagination;
    assert_eq!(pagination.page, 2);
    assert_eq!(pagination.page_size, 10);
}

#[test]
fn pagination_only_empty_values_fall_back_to_defaults() {
    let pagination = parse_list_query("page=&pageSize=").unwrap().pagination;
    assert_eq!(pagination, Pagination::default());
}

#[test]
fn pagination_only_search_params_are_ignored() {
    // 列表端点不接受检索条件，混进来的搜索参数既不生效也不报错
    let pagination = parse_list_query("musicName=ME!&q=hello&page=3")
        .unwrap()
        .pagination;
    assert_eq!(pagination.page, 3);
    assert_eq!(pagination.page_size, 50);
}

#[test]
fn pagination_only_rejects_zero_page() {
    let result = parse_list_query("page=0");
    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[test]
fn pagination_only_rejects_non_numeric_page() {
    let result = parse_list_query("page=abc");
    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[test]
fn pagination_only_rejects_oversized_page_size() {
    let result = parse_list_query("pageSize=101");
    assert!(matches!(result, Err(AppError::BadRequest(_))));
}

#[test]
fn list_query_defaults_sort_and_order() {
    let query = parse_list_query("").unwrap();
    assert_eq!(query.sort, ListSort::CreatedAt);
    assert_eq!(query.order, ListOrder::Desc);
}

#[test]
fn list_query_source_param() {
    use crate::core::models::LyricSource;

    assert_eq!(
        parse_list_query("source=private").unwrap().filter.source,
        Some(LyricSource::Private)
    );
    assert_eq!(
        parse_list_query("source=public").unwrap().filter.source,
        Some(LyricSource::Public)
    );
    // all 等价于不传
    assert_eq!(parse_list_query("source=all").unwrap().filter.source, None);
    assert_eq!(parse_list_query("").unwrap().filter.source, None);

    let result = parse_list_query("source=bogus");
    assert!(matches!(
        result,
        Err(AppError::BadRequest(ref msg))
            if msg == "'source' must be one of 'private', 'public', 'all', got 'bogus'."
    ));
}

#[test]
fn list_query_cursor_requires_explicit_source() {
    let result = parse_list_query("cursor=1768754400682_699269132670751");
    assert!(matches!(result, Err(AppError::BadRequest(_))));

    let ok = parse_list_query("cursor=1768754400682_699269132670751&source=public").unwrap();
    assert!(ok.cursor.is_some());
}

#[test]
fn list_query_parses_all_sort_values() {
    for (raw, expected) in [
        ("createdAt", ListSort::CreatedAt),
        ("id", ListSort::Id),
        ("musicName", ListSort::TrackName),
        ("artistName", ListSort::ArtistName),
        ("albumName", ListSort::AlbumName),
    ] {
        assert_eq!(
            parse_list_query(&format!("sort={raw}")).unwrap().sort,
            expected
        );
    }
}

#[test]
fn list_query_rejects_invalid_sort_and_order() {
    assert!(matches!(
        parse_list_query("sort=created_at"),
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        parse_list_query("order=down"),
        Err(AppError::BadRequest(_))
    ));
}

// --- 列表端点结构化过滤测试组 ---

#[test]
fn list_query_has_no_filter_by_default() {
    assert!(
        parse_list_query("page=2&sort=id")
            .unwrap()
            .filter
            .is_empty()
    );
}

#[test]
fn list_query_parses_author_filters() {
    let filter = parse_list_query("authorId=12345&authorUsername=someone")
        .unwrap()
        .filter;
    assert_eq!(filter.author_id.as_deref(), Some("12345"));
    assert_eq!(filter.author_username.as_deref(), Some("someone"));
    assert!(filter.has.is_empty());
    assert!(filter.missing.is_empty());
}

#[test]
fn list_query_keeps_author_values_verbatim() {
    // 与 /lyrics/search 的作者参数保持一致：不 trim、不改大小写，原样交给倒排索引比对
    let filter = parse_list_query("authorId=+Someone+&authorUsername=SomeOne")
        .unwrap()
        .filter;
    assert_eq!(filter.author_id.as_deref(), Some(" Someone "));
    assert_eq!(filter.author_username.as_deref(), Some("SomeOne"));
}

#[test]
fn list_query_repeated_author_param_keeps_last() {
    let filter = parse_list_query("authorId=first&authorId=second")
        .unwrap()
        .filter;
    assert_eq!(filter.author_id.as_deref(), Some("second"));
}

#[test]
fn list_query_parses_all_id_kind_values() {
    for &(raw, kind) in ID_KIND_MAP {
        let filter = parse_list_query(&format!("hasId={raw}")).unwrap().filter;
        assert!(filter.has.contains(kind), "hasId={raw} 未解析出 {kind:?}");
        assert!(filter.missing.is_empty());

        let filter = parse_list_query(&format!("missingId={raw}"))
            .unwrap()
            .filter;
        assert!(
            filter.missing.contains(kind),
            "missingId={raw} 未解析出 {kind:?}"
        );
        assert!(filter.has.is_empty());
    }
}

#[test]
fn list_query_comma_and_repeated_id_forms_agree() {
    let comma = parse_list_query("hasId=ncmMusicId,isrc").unwrap().filter;
    let repeated = parse_list_query("hasId=ncmMusicId&hasId=isrc")
        .unwrap()
        .filter;
    let mixed = parse_list_query("hasId=ncmMusicId,isrc&hasId=isrc")
        .unwrap()
        .filter;

    assert_eq!(comma.has, repeated.has);
    assert_eq!(comma.has, mixed.has);
    assert!(comma.has.contains(IdKind::NcmMusicId));
    assert!(comma.has.contains(IdKind::Isrc));
    assert!(!comma.has.contains(IdKind::SpotifyId));
}

#[test]
fn list_query_ignores_blank_id_filters() {
    for raw in ["hasId=", "hasId=+", "hasId=,", "missingId=,+,"] {
        let filter = parse_list_query(raw).unwrap().filter;
        assert!(filter.is_empty(), "{raw} 应当等同于没传");
    }
}

#[test]
fn list_query_rejects_unknown_id_kind() {
    let err = parse_list_query("hasId=ncm").unwrap_err();
    let AppError::BadRequest(message) = err else {
        panic!("未知取值应当是 400");
    };
    assert!(message.contains("'hasId'"), "{message}");
    assert!(message.contains("got 'ncm'"), "{message}");

    assert!(matches!(
        parse_list_query("hasId=ncmMusicId,nope"),
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        parse_list_query("missingId=platform"),
        Err(AppError::BadRequest(_))
    ));
}

#[test]
fn list_query_rejects_overlapping_has_and_missing() {
    let err = parse_list_query("hasId=ncmMusicId,isrc&missingId=isrc,spotifyId").unwrap_err();
    let AppError::BadRequest(message) = err else {
        panic!("hasId 与 missingId 相交应当是 400");
    };
    // 只报真正相交的那个取值，另外两个各自只出现在一侧，不算冲突
    assert!(message.contains("'isrc'"), "{message}");
    assert!(!message.contains("'ncmMusicId'"), "{message}");
    assert!(!message.contains("'spotifyId'"), "{message}");
}

#[test]
fn list_query_allows_disjoint_has_and_missing() {
    let filter = parse_list_query("hasId=ncmMusicId&missingId=isrc")
        .unwrap()
        .filter;
    assert!(filter.has.contains(IdKind::NcmMusicId));
    assert!(filter.missing.contains(IdKind::Isrc));
}

#[test]
fn id_kind_message_values_match_map() {
    assert_eq!(ID_KIND_MAP.len(), IdKind::ALL.len());

    for kind in IdKind::ALL {
        let names: Vec<&str> = ID_KIND_MAP
            .iter()
            .filter(|&&(_, mapped)| mapped == kind)
            .map(|&(name, _)| name)
            .collect();
        assert_eq!(names.len(), 1, "{kind:?} 的对外取值应当唯一");
        assert!(
            ID_KIND_VALUES.contains(&format!("'{}'", names[0])),
            "报错消息的取值清单里缺少 {kind:?}"
        );
    }
}

// --- 列表端点游标与增量分页测试组 ---

#[test]
fn list_query_parses_cursor() {
    let query = parse_list_query("cursor=1768754400682_699269132670751&source=public").unwrap();
    let cursor = query.cursor.unwrap();
    assert_eq!(cursor.timestamp, 1_768_754_400_682);
    assert_eq!(cursor.id.get(), 699_269_132_670_751);
}

#[test]
fn list_query_parses_before_as_cursor_alias() {
    let query = parse_list_query("before=1768754400682_699269132670751&source=private").unwrap();
    let cursor = query.cursor.unwrap();
    assert_eq!(cursor.timestamp, 1_768_754_400_682);
    assert_eq!(cursor.id.get(), 699_269_132_670_751);
}

#[test]
fn list_query_allows_identical_cursor_and_before() {
    let query = parse_list_query(
        "cursor=1768754400682_699269132670751&before=1768754400682_699269132670751&source=public",
    )
    .unwrap();
    assert!(query.cursor.is_some());
}

#[test]
fn list_query_rejects_conflicting_cursor_and_before() {
    let err = parse_list_query(
        "cursor=1768754400682_699269132670751&before=1768754400682_111111111111111",
    )
    .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));
}

#[test]
fn list_query_rejects_page_with_cursor() {
    let err = parse_list_query("page=2&cursor=1768754400682_699269132670751").unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));

    let err = parse_list_query("page=1&before=1768754400682_699269132670751").unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));
}

#[test]
fn list_query_rejects_cursor_with_non_created_at_sort() {
    let err = parse_list_query("sort=musicName&cursor=1768754400682_699269132670751").unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));

    let err = parse_list_query("sort=id&cursor=1768754400682_699269132670751").unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));
}

#[test]
fn list_query_rejects_malformed_cursor() {
    assert!(matches!(
        parse_list_query("cursor=abc"),
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        parse_list_query("cursor=123_"),
        Err(AppError::BadRequest(_))
    ));
}

#[test]
fn list_query_parses_since_and_until() {
    let query = parse_list_query("since=1000&until=2000").unwrap();
    assert_eq!(query.filter.since, Some(1000));
    assert_eq!(query.filter.until, Some(2000));
}

#[test]
fn list_query_rejects_since_greater_than_until() {
    let err = parse_list_query("since=2000&until=1000").unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));
}

#[test]
fn list_query_rejects_invalid_since_or_until() {
    assert!(matches!(
        parse_list_query("since=invalid"),
        Err(AppError::BadRequest(ref msg))
            if msg == "'since' must be a valid non-negative integer timestamp, got 'invalid'."
    ));
    assert!(matches!(
        parse_list_query("until=-1"),
        Err(AppError::BadRequest(ref msg))
            if msg == "'until' must be a valid non-negative integer timestamp, got '-1'."
    ));
}

/// 文案写的是 non-negative，0 必须被接受
#[test]
fn list_query_accepts_zero_since_and_until() {
    let query = parse_list_query("since=0&until=0").unwrap();
    assert_eq!(query.filter.since, Some(0));
    assert_eq!(query.filter.until, Some(0));
}

// --- LRCLIB Search 测试组 ---

#[test]
fn lrclib_search_no_params_returns_error() {
    let result = parse_search_query("", &LRCLIB_SEARCH_DIALECT);
    assert!(result.is_err());
}

#[test]
fn lrclib_search_q_only_success() {
    let (query, pagination) = parse_search_query("q=Taylor+Swift", &LRCLIB_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("Taylor Swift"));
    assert!(query.track_name.is_none());
    assert_eq!(pagination.page, 1);
    assert_eq!(pagination.page_size, 50);
}

#[test]
fn lrclib_search_snake_case_params_success() {
    let (query, _) = parse_search_query(
        "track_name=ME!&artist_name=Taylor+Swift",
        &LRCLIB_SEARCH_DIALECT,
    )
    .unwrap();
    assert!(query.global_keyword.is_none());
    assert_eq!(query.track_name.as_deref(), Some("ME!"));
    assert_eq!(query.artist_name.as_deref(), Some("Taylor Swift"));
}

#[test]
fn lrclib_search_ignores_duration_and_unsupported() {
    let (query, _) = parse_search_query(
        "track_name=ME!&duration=193&unknown=abc",
        &LRCLIB_SEARCH_DIALECT,
    )
    .unwrap();
    assert_eq!(query.track_name.as_deref(), Some("ME!"));
}

/// `duration` 被接受但当前不参与匹配
#[test]
fn lrclib_get_accepts_and_ignores_duration() {
    let with_duration = parse_search_query_exact(
        "track_name=ME!&artist_name=Taylor&duration=193",
        &LRCLIB_GET_DIALECT,
    )
    .unwrap();
    let without =
        parse_search_query_exact("track_name=ME!&artist_name=Taylor", &LRCLIB_GET_DIALECT).unwrap();
    assert_eq!(with_duration.track_name, without.track_name);
    assert_eq!(with_duration.artist_name, without.artist_name);
    assert_eq!(with_duration.album_name, without.album_name);
}

#[test]
fn lrclib_search_q_is_ignored_if_specific_fields_exist() {
    let (query, _) =
        parse_search_query("q=hello&track_name=world", &LRCLIB_SEARCH_DIALECT).unwrap();
    assert!(query.global_keyword.is_none());
    assert_eq!(query.track_name.as_deref(), Some("world"));
}

#[test]
fn lrclib_search_page_and_page_size_parsed() {
    let (query, pagination) =
        parse_search_query("q=hello&page=2&pageSize=25", &LRCLIB_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("hello"));
    assert_eq!(pagination.page, 2);
    assert_eq!(pagination.page_size, 25);
}

#[test]
fn lrclib_search_page_params_do_not_set_has_param_flag() {
    let result = parse_search_query("page=3&pageSize=10", &LRCLIB_SEARCH_DIALECT);
    assert!(result.is_err());
}

#[test]
fn lrclib_search_page_params_do_not_trigger_q_discard() {
    let (query, pagination) = parse_search_query("q=love&page=3", &LRCLIB_SEARCH_DIALECT).unwrap();
    assert_eq!(query.global_keyword.as_deref(), Some("love"));
    assert_eq!(pagination.page, 3);
}

// --- LRCLIB Get 精准匹配测试组 ---

#[test]
fn lrclib_get_missing_artist_returns_error() {
    let result = parse_search_query_exact("track_name=ME!", &LRCLIB_GET_DIALECT);
    assert!(result.is_err());
    if let Err(AppError::BadRequest(msg)) = result {
        assert!(msg.contains("Both 'track_name' and 'artist_name' are required"));
    }
}

#[test]
fn lrclib_get_missing_track_returns_error() {
    let result = parse_search_query_exact("artist_name=Taylor", &LRCLIB_GET_DIALECT);
    assert!(result.is_err());
}

#[test]
fn lrclib_get_valid_params_success() {
    let result =
        parse_search_query_exact("track_name=ME!&artist_name=Taylor", &LRCLIB_GET_DIALECT).unwrap();
    assert_eq!(result.track_name.as_deref(), Some("ME!"));
    assert_eq!(result.artist_name.as_deref(), Some("Taylor"));
}

#[test]
fn exact_source_search_keeps_lrc_lib_query_shape() {
    let query = parse_search_query_exact(
        "track_name=ME!&artist_name=Taylor&album_name=Lover&q=ignored&source=private",
        &LRCLIB_GET_DIALECT,
    )
    .unwrap();

    assert_eq!(query.source, Some(LyricSource::Private));
    assert_eq!(query.track_name.as_deref(), Some("ME!"));
    assert_eq!(query.artist_name.as_deref(), Some("Taylor"));
    assert_eq!(query.album_name.as_deref(), Some("Lover"));
    assert!(query.global_keyword.is_none());
}

// --- Get ID 测试组 ---

#[test]
fn get_no_params_returns_error() {
    let result = parse_get_query("");
    assert!(result.is_err());
}

#[test]
fn get_empty_id_returns_error() {
    let result = parse_get_query("spotifyId=");
    assert!(result.is_err());
}

#[test]
fn get_format_only_returns_error() {
    let result = parse_get_query("format=ttml");
    assert!(result.is_err());
}

#[test]
fn get_single_spotify_id() {
    let result = parse_get_query("spotifyId=abc123").unwrap();
    assert_eq!(result.id_query.spotify_ids, vec!["abc123"]);
    assert_eq!(result.format, "ttml");
}

#[test]
fn get_default_format_is_ttml() {
    let result = parse_get_query("ncmMusicId=111").unwrap();
    assert_eq!(result.format, "ttml");
}

#[test]
fn get_unsupported_format_returns_error() {
    let result = parse_get_query("spotifyId=abc&format=lrc");
    assert!(result.is_err());
}

#[test]
fn get_multiple_ids_same_type() {
    let result = parse_get_query("ncmMusicId=111&ncmMusicId=222").unwrap();
    assert_eq!(result.id_query.ncm_music_ids, vec!["111", "222"]);
}

#[test]
fn get_multiple_ids_different_types() {
    let result = parse_get_query("ncmMusicId=111&spotifyId=abc&isrc=XYZ").unwrap();
    assert_eq!(result.id_query.ncm_music_ids, vec!["111"]);
    assert_eq!(result.id_query.spotify_ids, vec!["abc"]);
    assert_eq!(result.id_query.isrcs, vec!["XYZ"]);
}

#[test]
fn get_all_id_types() {
    let result =
        parse_get_query("ncmMusicId=a&qqMusicId=b&appleMusicId=c&spotifyId=d&isrc=e").unwrap();
    assert_eq!(result.id_query.ncm_music_ids, vec!["a"]);
    assert_eq!(result.id_query.qq_music_ids, vec!["b"]);
    assert_eq!(result.id_query.apple_music_ids, vec!["c"]);
    assert_eq!(result.id_query.spotify_ids, vec!["d"]);
    assert_eq!(result.id_query.isrcs, vec!["e"]);
}

#[test]
fn get_filename_only() {
    let result = parse_get_query("filename=1768754400682-250306205-r6IrpmBd.ttml").unwrap();
    assert_eq!(
        result.id_query.filename,
        Some("1768754400682-250306205-r6IrpmBd.ttml".into())
    );
    assert!(result.id_query.ncm_music_ids.is_empty());
}

#[test]
fn get_filename_with_other_ids_ignored() {
    let result = parse_get_query("filename=a.ttml&ncmMusicId=111").unwrap();
    assert_eq!(result.id_query.filename, Some("a.ttml".into()));
    assert_eq!(result.id_query.ncm_music_ids, vec!["111"]);
}

#[test]
fn get_empty_filename_returns_error() {
    let result = parse_get_query("filename=");
    assert!(result.is_err());
}

#[test]
fn get_invalid_filename_extension_returns_error() {
    let result = parse_get_query("filename=1768754400682-250306205-r6IrpmBd.lrc");
    assert!(matches!(
        result,
        Err(AppError::BadRequest(ref msg))
            if msg == "Invalid filename: '1768754400682-250306205-r6IrpmBd.lrc'. Must end with '.ttml'."
    ));
}

/// 与上传接口一致，`.ttml` 后缀不区分大小写；文件名本身原样保留用于精确查找
#[test]
fn get_filename_extension_is_case_insensitive() {
    for name in ["a.TTML", "a.Ttml", "a.ttml"] {
        let result = parse_get_query(&format!("filename={name}")).unwrap();
        assert_eq!(result.id_query.filename.as_deref(), Some(name));
    }
}

#[test]
fn has_ttml_extension_ignores_case() {
    assert!(has_ttml_extension("x.ttml"));
    assert!(has_ttml_extension("x.TTML"));
    assert!(!has_ttml_extension("x.ttm"));
    assert!(!has_ttml_extension("x.lrc"));
    assert!(!has_ttml_extension("ttml"));
}

#[test]
fn get_valid_id_parsed_correctly() {
    let result = parse_get_query("id=269710089745311").unwrap();
    assert_eq!(
        result.id_query.id,
        Some(LyricId::from_u64(269_710_089_745_311).unwrap())
    );
}

#[test]
fn get_invalid_id_format_returns_error() {
    let result1 = parse_get_query("id=abc");
    assert!(result1.is_err());

    let result2 = parse_get_query("id=-123");
    assert!(result2.is_err());
}

#[test]
fn get_id_with_other_params() {
    let result = parse_get_query("id=12345&ncmMusicId=111").unwrap();
    assert_eq!(result.id_query.id, Some(LyricId::from_u64(12345).unwrap()));
    assert_eq!(result.id_query.ncm_music_ids, vec!["111"]);
}

#[test]
fn search_query_null_bytes_replaced_with_spaces() {
    let (query, _) = parse_search_query(
        "q=Kanye+West%00Travis+Scott%00Future",
        &NATIVE_SEARCH_DIALECT,
    )
    .unwrap();
    assert_eq!(
        query.global_keyword.as_deref(),
        Some("Kanye West Travis Scott Future")
    );
}

#[test]
fn search_query_null_bytes_only_skipped() {
    let result = parse_search_query("q=%00%00", &NATIVE_SEARCH_DIALECT);
    assert!(result.is_err());
}

#[test]
fn list_query_null_bytes_replaced_with_spaces() {
    let query = parse_list_query("authorUsername=foo%00bar").unwrap();
    assert_eq!(query.filter.author_username.as_deref(), Some("foo bar"));
}

#[test]
fn get_query_null_bytes_replaced_with_spaces() {
    let result = parse_get_query("appleMusicId=123%00456").unwrap();
    assert_eq!(result.id_query.apple_music_ids, vec!["123 456"]);
}
