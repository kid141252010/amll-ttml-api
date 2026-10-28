//! 每日文件的日界：北京时间（UTC+8，无夏令时，按固定偏移处理）
//!
//! 团队和用户主要在国内，「某一天」的问题正好对应一个文件。
//! `ts` 列本身始终是 UTC 毫秒，日界只决定一条请求落进哪个文件

use chrono::{
    DateTime,
    FixedOffset,
    NaiveDate,
    NaiveTime,
};

const BEIJING_OFFSET_SECS: i32 = 8 * 3600;
pub const DAY_MS: i64 = 86_400_000;
const FILE_SUFFIX: &str = ".parquet";

/// 该 UTC 毫秒时刻所在的北京时间日期
pub fn day_of(ts_ms: i64) -> Option<NaiveDate> {
    let beijing = FixedOffset::east_opt(BEIJING_OFFSET_SECS)?;
    DateTime::from_timestamp_millis(ts_ms).map(|t| t.with_timezone(&beijing).date_naive())
}

/// 北京时间某日 0 点对应的 UTC 毫秒
pub fn day_start_ms(day: NaiveDate) -> i64 {
    day.and_time(NaiveTime::MIN).and_utc().timestamp_millis()
        - i64::from(BEIJING_OFFSET_SECS) * 1000
}

/// 每日文件名，例如 `2026-09-26.parquet`
pub fn file_name(day: NaiveDate) -> String {
    format!("{}{FILE_SUFFIX}", day.format("%Y-%m-%d"))
}

/// [`file_name`] 的逆运算，只认严格符合格式的文件名
pub fn parse_file_name(name: &str) -> Option<NaiveDate> {
    let day = NaiveDate::parse_from_str(name.strip_suffix(FILE_SUFFIX)?, "%Y-%m-%d").ok()?;
    (file_name(day) == name).then_some(day)
}

#[cfg(test)]
mod tests;
