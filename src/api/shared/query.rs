use url::form_urlencoded;

use crate::core::{
    LyricId,
    error::AppError,
    list_query::{
        Cursor,
        IdKind,
        IdKindSet,
        ListFilter,
        ListOrder,
        ListQuery,
        ListSort,
    },
    models::{
        IdQuery,
        LyricSource,
        SearchQuery,
    },
    pagination::Pagination,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFieldTarget {
    GlobalKeyword,
    TrackName,
    ArtistName,
    AlbumName,
    LyricText,
    AuthorId,
    AuthorUsername,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchRequirement {
    /// 允许任意搜索参数，若 `q` 与非 `q` 共同存在则丢弃 `q`，用于原生接口
    AnyParamWithQFallback,
    /// 必须同时包含指定的每一个字段 (如 `track_name` 和 `artist_name`)，用于 `LrcLib` 接口
    ExactRequired(&'static [SearchFieldTarget]),
}

pub struct SearchDialect {
    pub field_map: &'static [(&'static str, SearchFieldTarget)],
    pub requirement: SearchRequirement,
    /// 只属于 `/v1/lyrics/list`、本端点明确拒绝（400）而非静默忽略的参数
    ///
    /// 免得调用方误以为游标、排序、时间过滤在检索端点上生效了
    pub list_only_params: &'static [&'static str],
}

/// `/v1/lyrics/list` 专有的参数，`/v1/lyrics/search` 收到时返回 400
///
/// 只列 list 有而 search 没有的参数：`page` / `pageSize` / `source` 两边都支持，
/// `authorId` / `authorUsername` 在 search 里是检索字段，都不在此列
const LIST_ONLY_PARAMS: &[&str] = &[
    "cursor",
    "before",
    "sort",
    "order",
    "since",
    "until",
    "hasId",
    "missingId",
];

/// `/v1/lyrics/search` 的请求参数映射
pub static NATIVE_SEARCH_DIALECT: SearchDialect = SearchDialect {
    field_map: &[
        ("q", SearchFieldTarget::GlobalKeyword),
        ("musicName", SearchFieldTarget::TrackName),
        ("artistName", SearchFieldTarget::ArtistName),
        ("albumName", SearchFieldTarget::AlbumName),
        ("lyricText", SearchFieldTarget::LyricText),
        ("authorId", SearchFieldTarget::AuthorId),
        ("authorUsername", SearchFieldTarget::AuthorUsername),
    ],
    requirement: SearchRequirement::AnyParamWithQFallback,
    list_only_params: LIST_ONLY_PARAMS,
};

/// `/v1/lrclib/search` 的请求参数映射
///
/// 除 LRCLIB 原版参数外，还接受本服务扩展的 `source` / `page` / `pageSize`，规则同
/// `/v1/lyrics/search`
pub static LRCLIB_SEARCH_DIALECT: SearchDialect = SearchDialect {
    field_map: &[
        ("q", SearchFieldTarget::GlobalKeyword),
        ("track_name", SearchFieldTarget::TrackName),
        ("artist_name", SearchFieldTarget::ArtistName),
        ("album_name", SearchFieldTarget::AlbumName),
    ],
    requirement: SearchRequirement::AnyParamWithQFallback,
    list_only_params: &[],
};

/// `/v1/lrclib/get` 的请求参数映射
///
/// LRCLIB 原版的 `duration` 参数被接受但当前不参与匹配：它不在映射表里，
/// 与其他未知参数一样被忽略（实现时长容差匹配的成本较高，暂未做）
///
/// 另接受本服务扩展的 `source` 来源过滤；单条精确匹配，不分页
pub static LRCLIB_GET_DIALECT: SearchDialect = SearchDialect {
    field_map: &[
        ("track_name", SearchFieldTarget::TrackName),
        ("artist_name", SearchFieldTarget::ArtistName),
        ("album_name", SearchFieldTarget::AlbumName),
    ],
    requirement: SearchRequirement::ExactRequired(&[
        SearchFieldTarget::TrackName,
        SearchFieldTarget::ArtistName,
    ]),
    list_only_params: &[],
};

pub fn parse_search_query(
    query_str: &str,
    dialect: &SearchDialect,
) -> Result<(SearchQuery, Pagination), AppError> {
    let mut parsed = parse_query_internal(query_str, dialect)?;

    if !parsed.has_any_param {
        return Err(AppError::BadRequest(
            "Missing valid search parameters.".into(),
        ));
    }

    parsed.query.source = parse_source(parsed.source_raw.as_deref())?;
    let pagination =
        Pagination::from_raw(parsed.page_raw.as_deref(), parsed.page_size_raw.as_deref())?;
    Ok((parsed.query, pagination))
}

pub fn parse_search_query_exact(
    query_str: &str,
    dialect: &SearchDialect,
) -> Result<SearchQuery, AppError> {
    let mut parsed = parse_query_internal(query_str, dialect)?;

    if let SearchRequirement::ExactRequired(reqs) = dialect.requirement {
        for req in reqs {
            let present = match req {
                SearchFieldTarget::TrackName => parsed.query.track_name.is_some(),
                SearchFieldTarget::ArtistName => parsed.query.artist_name.is_some(),
                SearchFieldTarget::AlbumName => parsed.query.album_name.is_some(),
                SearchFieldTarget::GlobalKeyword => parsed.query.global_keyword.is_some(),
                SearchFieldTarget::LyricText => parsed.query.lyric_text.is_some(),
                SearchFieldTarget::AuthorId => parsed.query.author_id.is_some(),
                SearchFieldTarget::AuthorUsername => parsed.query.author_username.is_some(),
            };
            if !present {
                return Err(AppError::BadRequest(
                    "Both 'track_name' and 'artist_name' are required for precise matching.".into(),
                ));
            }
        }
    }

    parsed.query.source = parse_source(parsed.source_raw.as_deref())?;
    Ok(parsed.query)
}

/// 解析 `source` 参数，检索端点与 `/v1/lyrics/list` 共用
///
/// 与 `sort` / `order` / `hasId` 一样区分大小写，只认小写取值。
/// `all` 与未传等价，都返回 `None`（不过滤）
fn parse_source(raw: Option<&str>) -> Result<Option<LyricSource>, AppError> {
    raw.map(str::trim).map_or_else(
        || Ok(None),
        |s| match s {
            "private" => Ok(Some(LyricSource::Private)),
            "public" => Ok(Some(LyricSource::Public)),
            "all" => Ok(None),
            _ => Err(AppError::BadRequest(format!(
                "'source' must be one of 'private', 'public', 'all', got '{s}'."
            ))),
        },
    )
}

/// `/v1/lyrics/list` 的 `hasId` / `missingId` 取值映射
///
/// 取值与 `/v1/lyrics/get` 的平台 ID 参数同名，两处指的是同一批标识
static ID_KIND_MAP: &[(&str, IdKind)] = &[
    ("ncmMusicId", IdKind::NcmMusicId),
    ("qqMusicId", IdKind::QqMusicId),
    ("appleMusicId", IdKind::AppleMusicId),
    ("spotifyId", IdKind::SpotifyId),
    ("isrc", IdKind::Isrc),
];

/// 报错消息里的取值清单，与 `sort` / `order` 一样硬编码
const ID_KIND_VALUES: &str = "'ncmMusicId', 'qqMusicId', 'appleMusicId', 'spotifyId', or 'isrc'";

/// 解析列表端点查询参数：分页、排序与结构化过滤
///
/// 列表端点不接受检索条件，无法识别的参数会被忽略。`sort` 默认使用 `createdAt`，`order` 默认使用
/// `desc`
///
/// 过滤维度之间、以及 `hasId` / `missingId` 各自的多个取值之间，全部是 AND
#[expect(clippy::too_many_lines)]
pub fn parse_list_query(query_str: &str) -> Result<ListQuery, AppError> {
    let mut page_raw: Option<String> = None;
    let mut page_size_raw: Option<String> = None;
    let mut sort_raw: Option<String> = None;
    let mut order_raw: Option<String> = None;
    let mut cursor_raw: Option<String> = None;
    let mut before_raw: Option<String> = None;
    let mut since_raw: Option<String> = None;
    let mut until_raw: Option<String> = None;
    let mut source_raw: Option<String> = None;
    let mut filter = ListFilter::default();

    for (key, value) in form_urlencoded::parse(query_str.as_bytes()) {
        let value = value.replace('\0', " ");
        if value.trim().is_empty() {
            continue;
        }

        match key.as_ref() {
            "page" => page_raw = Some(value),
            "pageSize" => page_size_raw = Some(value),
            "sort" => sort_raw = Some(value),
            "order" => order_raw = Some(value),
            "cursor" => cursor_raw = Some(value),
            "before" => before_raw = Some(value),
            "since" => since_raw = Some(value),
            "until" => until_raw = Some(value),
            "source" => source_raw = Some(value),
            "authorId" => filter.author_id = Some(value),
            "authorUsername" => filter.author_username = Some(value),
            "hasId" => collect_id_kinds("hasId", &value, &mut filter.has)?,
            "missingId" => collect_id_kinds("missingId", &value, &mut filter.missing)?,
            _ => {}
        }
    }

    let cursor_str = match (cursor_raw, before_raw) {
        (Some(c), Some(b)) if c != b => {
            return Err(AppError::BadRequest(
                "'cursor' and 'before' cannot both be specified with different values.".into(),
            ));
        }
        (Some(c), _) => Some(c),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    if page_raw.is_some() && cursor_str.is_some() {
        return Err(AppError::BadRequest(
            "'page' and 'cursor' cannot be used together.".into(),
        ));
    }

    let overlap = filter.has.intersection(filter.missing);
    if !overlap.is_empty() {
        return Err(AppError::BadRequest(format!(
            "'hasId' and 'missingId' must not overlap, but both list {}.",
            format_id_kinds(overlap)
        )));
    }

    let since = match since_raw.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Some(s.parse::<u64>().map_err(|_| {
            AppError::BadRequest(format!(
                "'since' must be a valid non-negative integer timestamp, got '{s}'."
            ))
        })?),
        _ => None,
    };
    let until = match until_raw.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Some(s.parse::<u64>().map_err(|_| {
            AppError::BadRequest(format!(
                "'until' must be a valid non-negative integer timestamp, got '{s}'."
            ))
        })?),
        _ => None,
    };

    if let (Some(s), Some(u)) = (since, until)
        && s > u
    {
        return Err(AppError::BadRequest(format!(
            "'since' ({s}) must not be greater than 'until' ({u})."
        )));
    }

    filter.since = since;
    filter.until = until;

    filter.source = parse_source(source_raw.as_deref())?;

    let pagination = Pagination::from_raw(page_raw.as_deref(), page_size_raw.as_deref())?;
    let sort = match sort_raw.as_deref().map(str::trim) {
        None => ListSort::default(),
        Some("createdAt") => ListSort::CreatedAt,
        Some("id") => ListSort::Id,
        Some("musicName") => ListSort::TrackName,
        Some("artistName") => ListSort::ArtistName,
        Some("albumName") => ListSort::AlbumName,
        Some(value) => {
            return Err(AppError::BadRequest(format!(
                "'sort' must be one of 'createdAt', 'id', 'musicName', 'artistName', or 'albumName', got '{value}'."
            )));
        }
    };
    let order = match order_raw.as_deref().map(str::trim) {
        None => ListOrder::default(),
        Some("desc") => ListOrder::Desc,
        Some("asc") => ListOrder::Asc,
        Some(value) => {
            return Err(AppError::BadRequest(format!(
                "'order' must be either 'desc' or 'asc', got '{value}'."
            )));
        }
    };

    let cursor = match cursor_str {
        Some(ref s) => {
            if sort != ListSort::CreatedAt {
                return Err(AppError::BadRequest(
                    "Cursor pagination is only supported with 'sort=createdAt'.".into(),
                ));
            }
            // 游标二分依赖 (timestamp, id) 全序；source=all 时私库优先会打乱它，
            // 因此游标分页必须显式指定 source=private 或 source=public
            if filter.source.is_none() {
                return Err(AppError::BadRequest(
                    "'cursor' pagination requires an explicit 'source=private' or 'source=public'."
                        .into(),
                ));
            }
            Some(Cursor::parse(s)?)
        }
        None => None,
    };

    Ok(ListQuery {
        pagination,
        cursor,
        sort,
        order,
        filter,
    })
}

/// 把一个 `hasId` / `missingId` 的值按逗号切开并并入 `set`
///
/// 逗号分隔（`hasId=ncmMusicId,isrc`）与重复传参（`hasId=ncmMusicId&hasId=isrc`）可以混用，
/// token 取并集。空 token 跳过，因此 `hasId=,` 与没传等价
fn collect_id_kinds(param: &str, value: &str, set: &mut IdKindSet) -> Result<(), AppError> {
    for token in value.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        let kind = ID_KIND_MAP
            .iter()
            .find(|(name, _)| *name == token)
            .map(|&(_, kind)| kind)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "'{param}' must be one of {ID_KIND_VALUES}, got '{token}'."
                ))
            })?;
        set.insert(kind);
    }

    Ok(())
}

/// 按 [`ID_KIND_MAP`] 的顺序把集合渲染回对外取值名，用于报错消息
fn format_id_kinds(set: IdKindSet) -> String {
    ID_KIND_MAP
        .iter()
        .filter(|&&(_, kind)| set.contains(kind))
        .map(|(name, _)| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

struct ParsedQueryParams {
    pub query: SearchQuery,
    pub has_any_param: bool,
    pub page_raw: Option<String>,
    pub page_size_raw: Option<String>,
    /// `source` 原始值，校验留给调用方（本函数不返回 `Result`）
    pub source_raw: Option<String>,
}

fn parse_query_internal(
    query_str: &str,
    dialect: &SearchDialect,
) -> Result<ParsedQueryParams, AppError> {
    let mut query = SearchQuery::default();
    let mut has_non_q_param = false;
    let mut has_q = false;

    let mut page_raw: Option<String> = None;
    let mut page_size_raw: Option<String> = None;
    let mut source_raw: Option<String> = None;

    for (k, v) in form_urlencoded::parse(query_str.as_bytes()) {
        let val = v.replace('\0', " ");
        if val.trim().is_empty() {
            continue;
        }

        let key_str = k.as_ref();

        if key_str == "page" {
            page_raw = Some(val);
            continue;
        }
        if key_str == "pageSize" {
            page_size_raw = Some(val);
            continue;
        }
        // source 只是过滤条件，不计入 has_any_param
        if key_str == "source" {
            source_raw = Some(val);
            continue;
        }
        if dialect.list_only_params.contains(&key_str) {
            return Err(AppError::BadRequest(format!(
                "'{key_str}' is only supported by '/lyrics/list'."
            )));
        }

        if let Some(&target) =
            dialect.field_map.iter().find_map(
                |(name, target)| {
                    if *name == key_str { Some(target) } else { None }
                },
            )
        {
            match target {
                SearchFieldTarget::GlobalKeyword => {
                    query.global_keyword = Some(val);
                    has_q = true;
                }
                SearchFieldTarget::TrackName => {
                    query.track_name = Some(val);
                    has_non_q_param = true;
                }
                SearchFieldTarget::ArtistName => {
                    query.artist_name = Some(val);
                    has_non_q_param = true;
                }
                SearchFieldTarget::AlbumName => {
                    query.album_name = Some(val);
                    has_non_q_param = true;
                }
                SearchFieldTarget::LyricText => {
                    query.lyric_text = Some(val);
                    has_non_q_param = true;
                }
                SearchFieldTarget::AuthorId => {
                    query.author_id = Some(val);
                    has_non_q_param = true;
                }
                SearchFieldTarget::AuthorUsername => {
                    query.author_username = Some(val);
                    has_non_q_param = true;
                }
            }
        }
    }

    if dialect.requirement == SearchRequirement::AnyParamWithQFallback && has_q && has_non_q_param {
        query.global_keyword = None;
    }

    let has_any_param = has_q || has_non_q_param;

    Ok(ParsedQueryParams {
        query,
        has_any_param,
        page_raw,
        page_size_raw,
        source_raw,
    })
}

/// 文件名是否以 `.ttml` 结尾（不区分大小写），上传与 `/v1/lyrics/get?filename=` 共用
pub fn has_ttml_extension(filename: &str) -> bool {
    filename.to_ascii_lowercase().ends_with(".ttml")
}

pub struct GetQuery {
    pub id_query: IdQuery,
    pub format: String,
}

pub fn parse_get_query(query_str: &str) -> Result<GetQuery, AppError> {
    let mut query = IdQuery::default();
    let mut has_param = false;
    let mut format = String::from("ttml");

    for (k, v) in form_urlencoded::parse(query_str.as_bytes()) {
        let val = v.replace('\0', " ");
        if val.trim().is_empty() {
            continue;
        }
        match k.as_ref() {
            "id" => {
                let parsed_id = LyricId::parse(&val)?;
                query.id = Some(parsed_id);
                has_param = true;
            }
            "filename" => {
                query.filename = Some(val);
                has_param = true;
            }
            "ncmMusicId" => {
                query.ncm_music_ids.push(val);
                has_param = true;
            }
            "qqMusicId" => {
                query.qq_music_ids.push(val);
                has_param = true;
            }
            "appleMusicId" => {
                query.apple_music_ids.push(val);
                has_param = true;
            }
            "spotifyId" => {
                query.spotify_ids.push(val);
                has_param = true;
            }
            "isrc" => {
                query.isrcs.push(val);
                has_param = true;
            }
            "format" => {
                format = val;
            }
            _ => {}
        }
    }

    if format != "ttml" {
        return Err(AppError::BadRequest(format!(
            "Unsupported format: '{format}'. Only 'ttml' is currently supported."
        )));
    }

    if let Some(ref filename) = query.filename
        && !has_ttml_extension(filename)
    {
        return Err(AppError::BadRequest(format!(
            "Invalid filename: '{filename}'. Must end with '.ttml'."
        )));
    }

    if has_param {
        Ok(GetQuery {
            id_query: query,
            format,
        })
    } else {
        Err(AppError::BadRequest(
            "At least one parameter is required (id, filename, ncmMusicId, qqMusicId, appleMusicId, spotifyId, isrc).".into(),
        ))
    }
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
