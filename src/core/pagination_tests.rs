use super::*;

#[test]
fn defaults_when_absent() {
    let p = Pagination::from_raw(None, None).unwrap();
    assert_eq!(p.page, 1);
    assert_eq!(p.page_size, 50);
    assert_eq!(p.offset(), 0);
    assert_eq!(p.take(), 50);
}

#[test]
fn parses_explicit_values() {
    let p = Pagination::from_raw(Some("3"), Some("20")).unwrap();
    assert_eq!(p.page, 3);
    assert_eq!(p.page_size, 20);
    assert_eq!(p.offset(), 40);
    assert_eq!(p.take(), 20);
}

#[test]
fn accepts_max_page_size() {
    let p = Pagination::from_raw(None, Some("100")).unwrap();
    assert_eq!(p.page_size, 100);
}

#[test]
fn rejects_page_size_above_max() {
    let err = Pagination::from_raw(None, Some("101"));
    assert!(err.is_err());
}

#[test]
fn rejects_zero_page() {
    assert!(Pagination::from_raw(Some("0"), None).is_err());
}

#[test]
fn rejects_zero_page_size() {
    assert!(Pagination::from_raw(None, Some("0")).is_err());
}

#[test]
fn rejects_non_numeric() {
    assert!(Pagination::from_raw(Some("abc"), None).is_err());
    assert!(Pagination::from_raw(None, Some("1.5")).is_err());
    assert!(Pagination::from_raw(Some("-1"), None).is_err());
}

#[test]
fn total_pages_rounds_up() {
    let p = Pagination::from_raw(None, Some("20")).unwrap();
    assert_eq!(p.total_pages(0), 0);
    assert_eq!(p.total_pages(1), 1);
    assert_eq!(p.total_pages(20), 1);
    assert_eq!(p.total_pages(21), 2);
}

#[test]
fn has_more_reflects_remaining_pages() {
    let first = Pagination::from_raw(Some("1"), Some("10")).unwrap();
    assert!(first.has_more(25));
    assert!(!first.has_more(10));
    assert!(!first.has_more(0));

    let last = Pagination::from_raw(Some("3"), Some("10")).unwrap();
    assert!(!last.has_more(25));
}

#[test]
fn huge_page_saturates_instead_of_overflowing() {
    let p = Pagination::from_raw(Some(&u64::MAX.to_string()), Some("100")).unwrap();
    assert_eq!(p.offset(), usize::MAX);
    assert!(!p.has_more(1000));
}

#[test]
fn test_paginate_combinator() {
    let data: Vec<i32> = (1..=25).collect();
    let pagination = Pagination::from_raw(Some("2"), Some("10")).unwrap();

    let paginated = paginate(data, pagination, |x| x * 2);

    assert_eq!(
        paginated.items,
        vec![22, 24, 26, 28, 30, 32, 34, 36, 38, 40]
    );
    assert_eq!(paginated.pagination.page, Some(2));
    assert_eq!(paginated.pagination.page_size, 10);
    assert_eq!(paginated.pagination.total, 25);
    assert_eq!(paginated.pagination.total_pages, Some(3));
    assert!(paginated.pagination.has_more);
    assert!(paginated.pagination.next_cursor.is_none());
}

#[test]
fn pagination_info_cursor_mode_serialization() {
    let info = PaginationInfo {
        page: None,
        page_size: 50,
        total: 100,
        total_pages: None,
        has_more: true,
        next_cursor: Some("1768754400682_699269132670751".into()),
    };
    let json = serde_json::to_value(&info).unwrap();
    assert!(json.get("page").is_none());
    assert!(json.get("totalPages").is_none());
    assert_eq!(json["pageSize"], 50);
    assert_eq!(json["total"], 100);
    assert_eq!(json["hasMore"], true);
    assert_eq!(json["nextCursor"], "1768754400682_699269132670751");
}
