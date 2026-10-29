use std::collections::{
    HashMap,
    HashSet,
};

use compact_str::CompactString;

use crate::core::{
    LyricId,
    list_query::ListFilter,
    matcher::{
        MatchType,
        PreparedQuery,
        rough_match,
        score_entry,
    },
    models::{
        IdQuery,
        LyricIndexDB,
        SearchQuery,
        SongEntry,
    },
};

#[derive(Debug, Clone)]
pub struct MetadataHit<'a> {
    pub entry: &'a SongEntry,
    pub score: MatchType,
}

impl LyricIndexDB {
    /// 用于 `/api/v1/lyrics/get` 接口，根据各种 ID 参数查找歌曲。
    ///
    /// 匹配优先级：
    /// 1. `id`：53 位 ID。如果传入了 `id`，将进行 O(1) 查找并直接返回，忽略其他所有参数。
    /// 2. `filename`：如果传入了 `filename`（且未传入 `id`），先用私库命名空间 ID
    ///    精确匹配，命中直接返回； 否则用公库命名空间 ID 精确匹配，忽略其他平台 ID。
    /// 3. 各个平台 ID：如果既未传入 `id` 也未传入 `filename`，则用 AND 交集严格匹配所有传入的平台
    ///    ID（如 `ncm_music_ids`、`spotify_ids`）。只有某个歌词同时具有传入的所有平台 ID
    ///    时，才返回。
    pub fn find_by_ids(&self, query: &IdQuery) -> Vec<usize> {
        if let Some(id) = query.id {
            return self
                .id_idx
                .get(&id)
                .map(|&idx| vec![idx])
                .unwrap_or_default();
        }

        if let Some(ref filename) = query.filename {
            // 私库优先：先用私库命名空间 ID 查找，命中直接返回
            let private_id = LyricId::from_private_filename(filename);
            if let Some(&idx) = self.id_idx.get(&private_id)
                && self.entries[idx].filename.as_str() == filename.as_str()
            {
                return vec![idx];
            }
            let derived_id = LyricId::from_filename(filename);
            return self
                .id_idx
                .get(&derived_id)
                .copied()
                .filter(|&idx| self.entries[idx].filename.as_str() == filename.as_str())
                .map(|idx| vec![idx])
                .unwrap_or_default();
        }

        let mut candidates: Option<Vec<usize>> = None;

        let mut apply_filter = |ids: &[String], idx_map: &HashMap<CompactString, Vec<usize>>| {
            for id in ids {
                let matched = idx_map.get(id.as_str()).cloned().unwrap_or_default();

                candidates = match candidates.take() {
                    None => Some(matched),
                    Some(mut existing) => {
                        existing.retain(|x| matched.contains(x));
                        Some(existing)
                    }
                };
            }
        };

        apply_filter(&query.ncm_music_ids, &self.ncm_idx);
        apply_filter(&query.qq_music_ids, &self.qq_idx);
        apply_filter(&query.apple_music_ids, &self.apple_idx);
        apply_filter(&query.spotify_ids, &self.spotify_idx);
        apply_filter(&query.isrcs, &self.isrc_idx);

        let mut result = candidates.unwrap_or_default();
        // 私库优先：先按 source 排序再按下标排序（保序去重）
        result.sort_by_key(|&idx| (self.entries[idx].source.rank(), idx));
        result.dedup();
        result
    }

    /// 根据元数据字段（歌名、歌手、专辑及作者等）进行检索与模糊打分
    ///
    /// 若查询未包含任何具体元数据的约束（如只使用 `lyricText`
    /// 进行搜索），将返回所有候选条目并统一赋予 `MatchType::Perfect` 分数
    pub fn search_by_fields(&self, query: &SearchQuery) -> Vec<MetadataHit<'_>> {
        let prepared = PreparedQuery::from_search_query(query);

        // 如果传了歌词作者 ID 和用户名，直接精确匹配再模糊打分
        //
        // 与 `list_candidates` 同一写法：`Option` 的 `Some` / `None` 只由参数有没有传决定，
        // 索引里查不到的作者是空候选，两个作者参数同时传时 AND 才不会退化成只按另一个筛
        let by_author_id: Option<HashSet<usize>> = prepared.author_id.as_ref().map(|id| {
            self.author_id_idx
                .get(id.as_str())
                .map(|indices| indices.iter().copied().collect())
                .unwrap_or_default()
        });
        let by_author_username: Option<HashSet<usize>> =
            prepared.author_username.as_ref().map(|username| {
                self.author_username_idx
                    .get(username.as_str())
                    .map(|indices| indices.iter().copied().collect())
                    .unwrap_or_default()
            });

        let candidates: Vec<usize> = match (by_author_id, by_author_username) {
            (Some(ids), Some(usernames)) => ids.intersection(&usernames).copied().collect(),
            (Some(ids), None) => ids.into_iter().collect(),
            (None, Some(usernames)) => usernames.into_iter().collect(),
            (None, None) => (0..self.entries.len()).collect(),
        };

        // 按来源过滤，作者精确匹配与普通分支统一在这里生效
        let candidates: Vec<usize> = match query.source {
            Some(source) => candidates
                .into_iter()
                .filter(|&idx| self.entries[idx].source == source)
                .collect(),
            None => candidates,
        };

        // 仅 author 过滤无文本字段时，候选集已是精确匹配结果，直接按时间戳降序返回
        if !prepared.has_text_fields() {
            let mut result: Vec<MetadataHit<'_>> = candidates
                .iter()
                .map(|&idx| MetadataHit {
                    entry: &self.entries[idx],
                    score: MatchType::Perfect,
                })
                .collect();
            result.sort_unstable_by(|a, b| {
                a.entry
                    .source
                    .rank()
                    .cmp(&b.entry.source.rank())
                    .then_with(|| b.entry.timestamp.cmp(&a.entry.timestamp))
                    .then_with(|| a.entry.id.cmp(&b.entry.id))
            });
            return result;
        }

        // 模糊打分排序
        let mut scored_results: Vec<MetadataHit<'_>> = candidates
            .iter()
            .filter_map(|&idx| {
                let entry = &self.entries[idx];
                if rough_match(&prepared, entry) {
                    let score = score_entry(&prepared, entry);
                    if score > MatchType::NoMatch {
                        Some(MetadataHit { entry, score })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        scored_results.sort_unstable_by(|a, b| {
            a.entry
                .source
                .rank()
                .cmp(&b.entry.source.rank())
                .then_with(|| b.score.cmp(&a.score))
                .then_with(|| b.entry.timestamp.cmp(&a.entry.timestamp))
                // id 全局唯一，兜底保证全序。lrclib_search 直接在这个顺序上翻页，
                // 少了它同分同时间戳的条目会在相邻页之间重复或漏掉。
                .then_with(|| a.entry.id.cmp(&b.entry.id))
        });

        scored_results
    }

    /// `/api/v1/lyrics/list` 的结构化过滤，返回命中条目在 [`Self::entries`] 里的下标
    ///
    /// 作者维度有倒排索引可用，先用它把候选集缩到该作者的投稿；`hasId` / `missingId`
    /// 的选择性太低（多数条目都有 ncm ID、都没有 ISRC），不值得为其建索引，
    /// 在候选集上逐条算掩码即可
    ///
    /// 返回顺序不作保证，排序由调用方负责
    #[must_use]
    pub fn list_candidates(&self, filter: &ListFilter) -> Vec<usize> {
        if filter.is_empty() {
            return (0..self.entries.len()).collect();
        }

        // 两个 `Option` 的 `Some` / `None` 只由「参数有没有传」决定，与索引查得到查不到无关：
        // 查不到的作者是空候选，而不是「该条件不生效」——否则两个作者参数同时传、
        // 其中一个查不到时，AND 会退化成只按另一个筛（`search_by_fields` 曾有这个 bug）
        let by_author_id: Option<Vec<usize>> = filter.author_id.as_ref().map(|author_id| {
            self.author_id_idx
                .get(author_id.as_str())
                .cloned()
                .unwrap_or_default()
        });
        let by_author_username: Option<HashSet<usize>> =
            filter.author_username.as_ref().map(|author_username| {
                self.author_username_idx
                    .get(author_username.as_str())
                    .map(|indices| indices.iter().copied().collect())
                    .unwrap_or_default()
            });

        let candidates: Vec<usize> = match (by_author_id, by_author_username) {
            (Some(ids), Some(usernames)) => ids
                .into_iter()
                .filter(|idx| usernames.contains(idx))
                .collect(),
            (Some(ids), None) => ids,
            (None, Some(usernames)) => usernames.into_iter().collect(),
            (None, None) => (0..self.entries.len()).collect(),
        };

        candidates
            .into_iter()
            .filter(|&idx| {
                let entry = &self.entries[idx];
                if let Some(source) = filter.source
                    && entry.source != source
                {
                    return false;
                }
                if let Some(since) = filter.since
                    && entry.timestamp < since
                {
                    return false;
                }
                if let Some(until) = filter.until
                    && entry.timestamp > until
                {
                    return false;
                }
                let present = entry.present_id_kinds();
                present.contains_all(filter.has) && present.contains_none(filter.missing)
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
