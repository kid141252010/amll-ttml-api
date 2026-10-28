use super::*;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn day_boundary_is_beijing_midnight() {
    // 2026-09-26T15:59:59.999Z 是北京时间 09-26 23:59:59.999
    let last_ms_of_26 = 1_790_438_399_999;
    assert_eq!(day_of(last_ms_of_26), Some(date("2026-09-26")));
    assert_eq!(day_of(last_ms_of_26 + 1), Some(date("2026-09-27")));
    assert_eq!(day_start_ms(date("2026-09-27")), last_ms_of_26 + 1);
}

#[test]
fn day_start_round_trips() {
    let day = date("2026-01-01");
    assert_eq!(day_of(day_start_ms(day)), Some(day));
    assert_eq!(day_of(day_start_ms(day) - 1), day.pred_opt());
    assert_eq!(
        day_start_ms(day) + DAY_MS,
        day_start_ms(day.succ_opt().unwrap())
    );
}

#[test]
fn file_name_round_trips_strictly() {
    let day = date("2026-09-05");
    assert_eq!(file_name(day), "2026-09-05.parquet");
    assert_eq!(parse_file_name("2026-09-05.parquet"), Some(day));

    assert_eq!(parse_file_name("2026-9-5.parquet"), None);
    assert_eq!(parse_file_name("2026-09-05.parquet.tmp"), None);
    assert_eq!(parse_file_name("../2026-09-05.parquet"), None);
    assert_eq!(parse_file_name("buffer.db"), None);
}
