use super::query::PreparedQuery;
use crate::{
    core::models::SongEntry,
    utils::string::contains_ignore_ascii_case,
};

pub fn rough_match(prepared: &PreparedQuery, entry: &SongEntry) -> bool {
    if let Some(ref q) = prepared.global_keyword {
        // 如果用户的超长 Query 里已经完整包含了数据库的歌名或歌手，
        // 则直接把这首歌送进精确排序里
        let is_track_wrapped = entry
            .track_names
            .iter()
            .any(|n| contains_ignore_ascii_case(q, n.as_str()));
        let is_artist_wrapped = entry
            .artist_names
            .iter()
            .any(|n| contains_ignore_ascii_case(q, n.as_str()));

        if !is_track_wrapped || !is_artist_wrapped {
            let mut all_tokens_matched = true;

            for token in &prepared.global_tokens {
                let mut token_matched = false;

                // 双向包含逻辑：
                // A: 数据库字段 包含 Token (常规情况：DB="Taylor Swift", Token="Taylor")
                // B: Token 包含 数据库字段 (极端情况：Token="夜曲周杰伦", DB="夜曲"。)
                // 同时限制 db_val.len() > 1 防止单字母泛滥
                let check_match = |db_val: &str| {
                    contains_ignore_ascii_case(db_val, token)
                        || (db_val.len() > 1 && contains_ignore_ascii_case(token, db_val))
                };

                // 只要当前 token 命中了任意一个字段，该 token 就算验证通过
                if entry.track_names.iter().any(|n| check_match(n.as_str()))
                    || entry.artist_names.iter().any(|n| check_match(n.as_str()))
                    || entry.album_names.iter().any(|n| check_match(n.as_str()))
                {
                    token_matched = true;
                }

                // 如果有任何一个 token 完全没有匹配上，说明这首歌完全不符合用户的混合查询
                if !token_matched {
                    all_tokens_matched = false;
                    break;
                }
            }

            if !all_tokens_matched {
                return false;
            }
        }
    }

    // 具体字段的过滤为包含和 AND 关系
    if let Some(ref val) = prepared.track_name
        && !entry
            .normalized_track_names
            .iter()
            .any(|n| contains_ignore_ascii_case(n.as_str(), val))
    {
        return false;
    }

    if let Some(ref val) = prepared.artist_name
        && !entry
            .normalized_artist_names
            .iter()
            .any(|n| contains_ignore_ascii_case(n.as_str(), val))
    {
        return false;
    }

    if let Some(ref val) = prepared.album_name
        && !entry
            .normalized_album_names
            .iter()
            .any(|n| contains_ignore_ascii_case(n.as_str(), val))
    {
        return false;
    }

    // ID 和用户名严格相等才算匹配
    if let Some(ref val) = prepared.author_id
        && !entry.author_ids.iter().any(|n| n.as_str() == val)
    {
        return false;
    }
    if let Some(ref val) = prepared.author_username
        && !entry.author_usernames.iter().any(|n| n.as_str() == val)
    {
        return false;
    }

    true
}

#[cfg(test)]
#[path = "rough_tests.rs"]
mod tests;
