use std::collections::HashMap;

use crate::core::{
    LyricId,
    matcher::MatchType,
    models::{
        LyricHit,
        LyricIndexDB,
        LyricMatchField,
        SongEntry,
    },
    repository::MetadataHit,
};

pub struct SearchHit<'a> {
    pub entry: &'a SongEntry,
    pub metadata_score: MatchType,
    pub lyric_hit: Option<LyricHit>,
}

/// 私库优先的排序键：数值越小越靠前
const fn source_rank_key(hit: &SearchHit) -> u8 {
    hit.entry.source.rank()
}

pub fn merge_and_sort_hits<'a>(
    db: &'a LyricIndexDB,
    metadata_hits: Vec<MetadataHit<'a>>,
    lyric_hits: Vec<LyricHit>,
    is_explicit_lyric: bool,
) -> Vec<SearchHit<'a>> {
    let mut merged_map: HashMap<LyricId, SearchHit<'a>> = HashMap::new();

    if is_explicit_lyric {
        let is_full_metadata = metadata_hits.len() == db.entries.len();
        if is_full_metadata {
            for l_hit in lyric_hits {
                if let Some(&idx) = db.id_idx.get(&l_hit.id) {
                    merged_map.insert(
                        l_hit.id,
                        SearchHit {
                            entry: &db.entries[idx],
                            metadata_score: MatchType::Perfect,
                            lyric_hit: Some(l_hit),
                        },
                    );
                }
            }
        } else {
            let meta_map: HashMap<LyricId, MatchType> = metadata_hits
                .into_iter()
                .map(|h| (h.entry.id, h.score))
                .collect();

            for l_hit in lyric_hits {
                if let Some(&m_score) = meta_map.get(&l_hit.id)
                    && let Some(&idx) = db.id_idx.get(&l_hit.id)
                {
                    merged_map.insert(
                        l_hit.id,
                        SearchHit {
                            entry: &db.entries[idx],
                            metadata_score: m_score,
                            lyric_hit: Some(l_hit),
                        },
                    );
                }
            }
        }
    } else {
        for m_hit in metadata_hits {
            merged_map.insert(
                m_hit.entry.id,
                SearchHit {
                    entry: m_hit.entry,
                    metadata_score: m_hit.score,
                    lyric_hit: None,
                },
            );
        }

        for l_hit in lyric_hits {
            if let Some(hit) = merged_map.get_mut(&l_hit.id) {
                hit.lyric_hit = Some(l_hit);
            } else if let Some(&idx) = db.id_idx.get(&l_hit.id) {
                merged_map.insert(
                    l_hit.id,
                    SearchHit {
                        entry: &db.entries[idx],
                        metadata_score: MatchType::NoMatch,
                        lyric_hit: Some(l_hit),
                    },
                );
            }
        }
    }

    let mut sorted_hits: Vec<SearchHit<'a>> = merged_map.into_values().collect();

    // 计算命中标的的 5 级优先级桶:
    // - Level 5 (元数据强命中): `metadata_score` >= `MatchType::Medium`
    //   歌名/歌手/专辑精准或高匹配，置信度最高，优先排在首位
    // - Level 4 (元数据弱命中 + 歌词命中): `metadata_score` > `MatchType::NoMatch` && `has_lyric`
    //   元数据有弱模糊匹配且歌词正文同时也命中，两路互相印证
    // - Level 3 (主歌词正文命中): `lyric_hit.field` == `LyricMatchField::MainLyric`
    //   凭借主歌词正文全文检索命中，权重高于伴唱/和声
    // - Level 2 (仅元数据弱命中): `metadata_score` > `MatchType::NoMatch`
    //   仅元数据有弱模糊匹配，且无歌词正文命中
    // - Level 1 (仅背景人声歌词命中): `has_lyric` (`field` == `LyricMatchField::BackgroundVocal`)
    //   仅在背景人声歌词中检索到匹配
    // - Level 0 (无匹配兜底): 兜底其他情况
    let get_priority = |hit: &SearchHit| -> u8 {
        let has_lyric = hit.lyric_hit.is_some();
        let is_main = hit
            .lyric_hit
            .as_ref()
            .is_some_and(|l| l.field == LyricMatchField::MainLyric);
        let meta_strong = hit.metadata_score >= MatchType::Medium;
        let meta_exists = hit.metadata_score > MatchType::NoMatch;

        if meta_strong {
            return 5;
        } else if meta_exists && has_lyric {
            return 4;
        } else if is_main {
            return 3;
        } else if meta_exists {
            return 2;
        } else if has_lyric {
            return 1;
        }
        0
    };

    sorted_hits.sort_unstable_by(|a, b| {
        let p_a = get_priority(a);
        let p_b = get_priority(b);

        // 私库优先：两级排序键 (source_rank, 原有规则)，不许用加分法
        source_rank_key(a)
            .cmp(&source_rank_key(b))
            .then_with(|| p_b.cmp(&p_a))
            .then_with(|| {
                let r_a = a.lyric_hit.as_ref().map_or(1000.0, |l| l.rank);
                let r_b = b.lyric_hit.as_ref().map_or(1000.0, |l| l.rank);
                r_a.total_cmp(&r_b)
            })
            .then_with(|| b.metadata_score.cmp(&a.metadata_score))
            .then_with(|| b.entry.timestamp.cmp(&a.entry.timestamp))
            // 候选来自 HashMap，迭代顺序不确定；id 全局唯一，补上它让比较成为
            // 全序，否则同分同时间戳的条目会在相邻页之间重复或漏掉
            .then_with(|| a.entry.id.cmp(&b.entry.id))
    });

    sorted_hits
}

#[cfg(test)]
mod tests;
