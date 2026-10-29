use std::collections::HashMap;

use compact_str::CompactString;
use serde::{
    Deserialize,
    Serialize,
};

use crate::core::{
    LyricId,
    list_query::{
        IdKind,
        IdKindSet,
    },
};

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RawIndexEntry {
    pub raw_lyric_file: String,
    pub metadata: Vec<(String, Vec<String>)>,
}

/// 歌词条目来源：公库（上游 amll-ttml-db 同步）或私库（用户上传）
///
/// 读路径按 `rank()` 排序，私库永远排在公库前面
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LyricSource {
    #[default]
    Public,
    Private,
}

impl LyricSource {
    /// 排序键：私库 0，公库 1，数值越小越优先
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Private => 0,
            Self::Public => 1,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

impl From<&str> for LyricSource {
    fn from(s: &str) -> Self {
        match s {
            "private" => Self::Private,
            _ => Self::Public,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SongEntry {
    pub id: LyricId,
    pub filename: CompactString,
    pub timestamp: u64,
    pub source: LyricSource,

    pub track_names: Box<[CompactString]>,
    pub artist_names: Box<[CompactString]>,
    pub album_names: Box<[CompactString]>,

    pub normalized_track_names: Box<[CompactString]>,
    pub normalized_artist_names: Box<[CompactString]>,
    pub normalized_album_names: Box<[CompactString]>,

    pub ncm_music_ids: Box<[CompactString]>,
    pub qq_music_ids: Box<[CompactString]>,
    pub apple_music_ids: Box<[CompactString]>,
    pub spotify_ids: Box<[CompactString]>,

    pub isrcs: Box<[CompactString]>,

    pub author_ids: Box<[CompactString]>,
    pub author_usernames: Box<[CompactString]>,
}

impl SongEntry {
    /// 本条目实际携带了哪些外部标识，供 `/lyrics/list` 的 `hasId` / `missingId` 过滤判定
    ///
    /// 判据是「对外响应里那个数组非空」，与客户端看到的 `ncmMusicIds` 等字段一致
    #[must_use]
    pub fn present_id_kinds(&self) -> IdKindSet {
        let mut set = IdKindSet::empty();
        for (kind, ids) in [
            (IdKind::NcmMusicId, &self.ncm_music_ids),
            (IdKind::QqMusicId, &self.qq_music_ids),
            (IdKind::AppleMusicId, &self.apple_music_ids),
            (IdKind::SpotifyId, &self.spotify_ids),
            (IdKind::Isrc, &self.isrcs),
        ] {
            if !ids.is_empty() {
                set.insert(kind);
            }
        }
        set
    }
}

#[derive(Default)]
pub struct LyricIndexDB {
    pub entries: Vec<SongEntry>,
    pub id_idx: HashMap<LyricId, usize>,

    pub ncm_idx: HashMap<CompactString, Vec<usize>>,
    pub qq_idx: HashMap<CompactString, Vec<usize>>,
    pub apple_idx: HashMap<CompactString, Vec<usize>>,
    pub spotify_idx: HashMap<CompactString, Vec<usize>>,

    pub isrc_idx: HashMap<CompactString, Vec<usize>>,

    pub author_id_idx: HashMap<CompactString, Vec<usize>>,
    pub author_username_idx: HashMap<CompactString, Vec<usize>>,
}

impl LyricIndexDB {
    /// 由条目列表组装全部倒排索引
    #[must_use]
    pub fn from_entries(entries: Vec<SongEntry>) -> Self {
        let mut index = Self {
            entries: Vec::with_capacity(entries.len()),
            ..Self::default()
        };

        for song in entries {
            let i = index.entries.len();
            index.id_idx.insert(song.id, i);

            for id in &song.ncm_music_ids {
                index.ncm_idx.entry(id.clone()).or_default().push(i);
            }
            for id in &song.qq_music_ids {
                index.qq_idx.entry(id.clone()).or_default().push(i);
            }
            for id in &song.apple_music_ids {
                index.apple_idx.entry(id.clone()).or_default().push(i);
            }
            for id in &song.spotify_ids {
                index.spotify_idx.entry(id.clone()).or_default().push(i);
            }
            for id in &song.isrcs {
                index.isrc_idx.entry(id.clone()).or_default().push(i);
            }
            for id in &song.author_ids {
                index.author_id_idx.entry(id.clone()).or_default().push(i);
            }
            for id in &song.author_usernames {
                index
                    .author_username_idx
                    .entry(id.clone())
                    .or_default()
                    .push(i);
            }

            index.entries.push(song);
        }

        index
    }
}

#[derive(Default)]
pub struct IdQuery {
    pub id: Option<LyricId>,
    pub filename: Option<String>,

    pub ncm_music_ids: Vec<String>,
    pub qq_music_ids: Vec<String>,
    pub apple_music_ids: Vec<String>,
    pub spotify_ids: Vec<String>,

    pub isrcs: Vec<String>,
}

impl IdQuery {
    /// 是否为基于强唯一标识（ID 或文件名）的精确查询
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        self.id.is_some() || self.filename.is_some()
    }
}

#[derive(Default)]
pub struct SearchQuery {
    pub global_keyword: Option<String>,

    pub track_name: Option<String>,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
    pub lyric_text: Option<String>,

    pub author_id: Option<String>,
    pub author_username: Option<String>,

    /// 来源过滤：`None` 表示公私库都要（`source=all` 或未传）
    pub source: Option<LyricSource>,
}

/// FTS 正文检索的结构化过滤条件，下推进 SQL 与 MATCH 同时生效
///
/// 语义与 [`LyricIndexDB::search_by_fields`] 的同名条件一致：各维度之间 AND，
/// 作者精确且大小写敏感，`None` 表示该维度不限
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FtsFilter {
    pub source: Option<LyricSource>,
    pub author_id: Option<String>,
    pub author_username: Option<String>,
}

impl FtsFilter {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.source.is_none() && self.author_id.is_none() && self.author_username.is_none()
    }
}

impl From<&SearchQuery> for FtsFilter {
    fn from(query: &SearchQuery) -> Self {
        Self {
            source: query.source,
            author_id: query.author_id.clone(),
            author_username: query.author_username.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricMatchField {
    MainLyric,
    BackgroundVocal,
}

#[derive(Debug, Clone)]
pub struct LyricHit {
    pub id: LyricId,
    pub rank: f64,
    pub field: LyricMatchField,
    pub snippet: Option<String>,
}

/// `lyrics/search` 的一条命中结果，包含条目本体和可选的歌词正文命中信息
#[derive(Debug, Clone)]
pub struct LyricSearchResult {
    pub entry: SongEntry,
    pub lyric_hit: Option<LyricHit>,
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
