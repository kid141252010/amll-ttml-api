use compact_str::CompactString;
use serde::Serialize;

use crate::core::error::AppError;

pub const DEFAULT_PAGE: u64 = 1;
pub const DEFAULT_PAGE_SIZE: u64 = 50;
pub const MAX_PAGE_SIZE: u64 = 100;

/// 搜索接口的分页参数。
///
/// `page` 从 1 起算，`page_size` 上限为 [`MAX_PAGE_SIZE`]
///
/// 由 extractor 层从搜索查询参数构造，服务层只消费
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    pub page: u64,
    pub page_size: u64,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: DEFAULT_PAGE,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}

impl Pagination {
    /// 从搜索查询参数的原始值构造，`None` 表示用户未传相关分页参数
    ///
    /// # Errors
    ///
    /// 非正整数、或 `page_size` 超过 [`MAX_PAGE_SIZE`] 时返回 [`AppError::BadRequest`]
    pub fn from_raw(page: Option<&str>, page_size: Option<&str>) -> Result<Self, AppError> {
        let page = match page {
            Some(raw) => parse_positive(raw, "page")?,
            None => DEFAULT_PAGE,
        };

        let page_size = match page_size {
            Some(raw) => {
                let value = parse_positive(raw, "pageSize")?;
                if value > MAX_PAGE_SIZE {
                    return Err(AppError::BadRequest(format!(
                        "'pageSize' must not exceed {MAX_PAGE_SIZE}."
                    )));
                }
                value
            }
            None => DEFAULT_PAGE_SIZE,
        };

        Ok(Self { page, page_size })
    }

    /// 要跳过的条目数，用于 [`Iterator::skip`]
    #[must_use]
    pub fn offset(&self) -> usize {
        let offset = self.page.saturating_sub(1).saturating_mul(self.page_size);
        usize::try_from(offset).unwrap_or(usize::MAX)
    }

    /// 本页最多取几条，用于 [`Iterator::take`]
    #[must_use]
    pub fn take(&self) -> usize {
        usize::try_from(self.page_size).unwrap_or(usize::MAX)
    }

    /// 给定总数时的总页数，总数为 0 时是 0 页
    #[must_use]
    pub const fn total_pages(&self, total: u64) -> u64 {
        total.div_ceil(self.page_size)
    }

    /// 当前页之后是否还有内容
    #[must_use]
    pub const fn has_more(&self, total: u64) -> bool {
        self.page < self.total_pages(total)
    }
}

fn parse_positive(raw: &str, field: &str) -> Result<u64, AppError> {
    let value: u64 = raw
        .trim()
        .parse()
        .map_err(|_| AppError::BadRequest(format!("'{field}' must be a positive integer.")))?;

    if value == 0 {
        return Err(AppError::BadRequest(format!(
            "'{field}' must be at least 1."
        )));
    }

    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginationInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u64>,
    pub page_size: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<u64>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<CompactString>,
}

/// 分页组合器输出结果，包含当前页的切片数据与完整的分页元数据信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub pagination: PaginationInfo,
}

/// 泛型组合器：对全量 slice 或 Iterator 进行安全的 offset/take 内存分页，并自动构建完整的
/// [`PaginationInfo`] 元数据
pub fn paginate<T, I, F, R>(items: I, pagination: Pagination, mut transform: F) -> Paginated<R>
where
    I: IntoIterator<Item = T>,
    I::IntoIter: ExactSizeIterator,
    F: FnMut(T) -> R,
{
    let iter = items.into_iter();
    let total = u64::try_from(iter.len()).unwrap_or(u64::MAX);

    let paged_items: Vec<R> = iter
        .skip(pagination.offset())
        .take(pagination.take())
        .map(&mut transform)
        .collect();

    Paginated {
        items: paged_items,
        pagination: PaginationInfo {
            page: Some(pagination.page),
            page_size: pagination.page_size,
            total,
            total_pages: Some(pagination.total_pages(total)),
            has_more: pagination.has_more(total),
            next_cursor: None,
        },
    }
}

#[cfg(test)]
#[path = "pagination_tests.rs"]
mod tests;
