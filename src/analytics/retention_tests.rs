use std::fs;

use super::*;
use crate::analytics::day::file_name;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn write_daily(dir: &Path, day: &str, bytes: usize) {
    fs::write(dir.join(file_name(date(day))), vec![0_u8; bytes]).unwrap();
}

fn remaining(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn removes_files_past_the_retention_window() {
    let root = tempfile::tempdir().unwrap();
    let parquet = root.path().join("parquet");
    fs::create_dir(&parquet).unwrap();
    for day in ["2026-06-28", "2026-06-29", "2026-09-26"] {
        write_daily(&parquet, day, 10);
    }

    let policy = RetentionPolicy {
        retention_days: 90,
        max_total_bytes: u64::MAX,
    };
    apply(root.path(), &parquet, date("2026-09-27"), &policy).unwrap();

    // 09-27 往前 90 天是 06-29，当天仍在窗口内
    assert_eq!(
        remaining(&parquet),
        ["2026-06-29.parquet", "2026-09-26.parquet"]
    );
}

#[test]
fn size_cap_removes_oldest_first_and_counts_the_buffer() {
    let root = tempfile::tempdir().unwrap();
    let parquet = root.path().join("parquet");
    fs::create_dir(&parquet).unwrap();
    fs::write(root.path().join("buffer.db"), vec![0_u8; 50]).unwrap();
    for day in ["2026-09-24", "2026-09-25", "2026-09-26"] {
        write_daily(&parquet, day, 100);
    }

    // 50 + 3 × 100 = 350，上限 240：删一个剩 250 仍超，再删一个剩 150
    let policy = RetentionPolicy {
        retention_days: 90,
        max_total_bytes: 240,
    };
    apply(root.path(), &parquet, date("2026-09-27"), &policy).unwrap();

    assert_eq!(remaining(&parquet), ["2026-09-26.parquet"]);
    assert!(root.path().join("buffer.db").exists(), "缓冲库不参与删除");
}

#[test]
fn keeps_fresh_temp_files_and_unknown_names() {
    let root = tempfile::tempdir().unwrap();
    let parquet = root.path().join("parquet");
    fs::create_dir(&parquet).unwrap();
    fs::write(parquet.join(".2026-09-26.parquet.3000-abc.tmp"), b"x").unwrap();
    fs::write(parquet.join("notes.txt"), b"x").unwrap();

    let policy = RetentionPolicy {
        retention_days: 90,
        max_total_bytes: u64::MAX,
    };
    apply(root.path(), &parquet, date("2026-09-27"), &policy).unwrap();

    // 刚写的临时文件可能正被另一个实例使用，不能删
    assert_eq!(
        remaining(&parquet),
        [".2026-09-26.parquet.3000-abc.tmp", "notes.txt"]
    );
}
