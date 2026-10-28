//! `/lyrics/list` 的查询参数模型：分页、排序与结构化过滤
//!
//! 对外取值与本模块类型的映射（`sort=createdAt`、`hasId=ncmMusicId` 等）留在
//! `api::shared::query`，与那边的 dialect 数据表同居一处；本模块只描述语义

use compact_str::CompactString;

use crate::core::{
    LyricId,
    error::AppError,
    models::{
        LyricSource,
        SongEntry,
    },
    pagination::Pagination,
};

/// 列表排序维度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListSort {
    #[default]
    CreatedAt,
    Id,
    TrackName,
    ArtistName,
    AlbumName,
}

/// 列表排序方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListOrder {
    #[default]
    Desc,
    Asc,
}

/// `/lyrics/list` 的复合游标，由条目创建时刻（毫秒）与 53 位唯一 ID 组成
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub timestamp: u64,
    pub id: LyricId,
}

impl Cursor {
    #[must_use]
    pub const fn from_entry(entry: &SongEntry) -> Self {
        Self {
            timestamp: entry.timestamp,
            id: entry.id,
        }
    }

    pub fn parse(raw: &str) -> Result<Self, AppError> {
        let (ts_str, id_str) = raw.split_once('_').ok_or_else(|| {
            AppError::BadRequest(format!(
                "Invalid cursor format: '{raw}'. Expected '<timestamp>_<id>'."
            ))
        })?;
        let timestamp: u64 = ts_str.parse().map_err(|_| {
            AppError::BadRequest(format!(
                "Invalid cursor timestamp: '{ts_str}'. Expected unsigned integer."
            ))
        })?;
        let id = LyricId::parse(id_str)?;
        Ok(Self { timestamp, id })
    }

    #[must_use]
    pub fn to_compact_string(self) -> CompactString {
        use std::fmt::Write;
        let mut s = CompactString::default();
        let _ = write!(s, "{}_{}", self.timestamp, self.id.get());
        s
    }
}

/// 歌词条目可以携带的外部标识种类
///
/// 与 [`SongEntry`](crate::core::models::SongEntry) 的五个标识字段一一对应
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKind {
    NcmMusicId,
    QqMusicId,
    AppleMusicId,
    SpotifyId,
    Isrc,
}

impl IdKind {
    pub const ALL: [Self; 5] = [
        Self::NcmMusicId,
        Self::QqMusicId,
        Self::AppleMusicId,
        Self::SpotifyId,
        Self::Isrc,
    ];

    const fn bit(self) -> u8 {
        match self {
            Self::NcmMusicId => 0b0000_0001,
            Self::QqMusicId => 0b0000_0010,
            Self::AppleMusicId => 0b0000_0100,
            Self::SpotifyId => 0b0000_1000,
            Self::Isrc => 0b0001_0000,
        }
    }
}

/// [`IdKind`] 的位集合
///
/// 三种语义都归约到同一个掩码上：一条歌词**实际携带**哪些标识、`hasId` 要求**必须有**哪些、
/// `missingId` 要求**必须缺**哪些。于是过滤判定退化成两次位运算
/// （[`contains_all`](Self::contains_all) / [`contains_none`](Self::contains_none)），
/// 两个参数是否撞了同一个取值也只是一次求交
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IdKindSet(u8);

impl IdKindSet {
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn insert(&mut self, kind: IdKind) {
        self.0 |= kind.bit();
    }

    #[must_use]
    pub const fn contains(self, kind: IdKind) -> bool {
        self.0 & kind.bit() != 0
    }

    /// 是否包含 `other` 的全部成员。`other` 为空集时恒真，因此未传 `hasId` 自然不构成约束
    #[must_use]
    pub const fn contains_all(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// 是否与 `other` 完全不相交。`other` 为空集时恒真，因此未传 `missingId` 自然不构成约束
    #[must_use]
    pub const fn contains_none(self, other: Self) -> bool {
        self.0 & other.0 == 0
    }

    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

/// `/lyrics/list` 的结构化过滤条件
///
/// 各维度之间、以及 `has` / `missing` 内部的多个取值之间，全部是 AND
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListFilter {
    pub author_id: Option<String>,
    pub author_username: Option<String>,
    pub has: IdKindSet,
    pub missing: IdKindSet,
    pub since: Option<u64>,
    pub until: Option<u64>,
    /// 私库/公库过滤：`None` 表示全部（`source=all` 或未传）
    pub source: Option<LyricSource>,
}

impl ListFilter {
    /// 是否不含任何过滤条件，用于让未过滤的列表请求走全表快路径
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.author_id.is_none()
            && self.author_username.is_none()
            && self.has.is_empty()
            && self.missing.is_empty()
            && self.since.is_none()
            && self.until.is_none()
            && self.source.is_none()
    }
}

/// `/lyrics/list` 的完整查询参数
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListQuery {
    pub pagination: Pagination,
    pub cursor: Option<Cursor>,
    pub sort: ListSort,
    pub order: ListOrder,
    pub filter: ListFilter,
}

#[cfg(test)]
#[path = "list_query_tests.rs"]
mod tests;
