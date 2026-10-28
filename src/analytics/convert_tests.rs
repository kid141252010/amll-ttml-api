use std::{
    fs::File,
    path::Path,
};

use arrow_array::{
    Array,
    RecordBatch,
    StringArray,
    TimestampMillisecondArray,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use sea_orm::{
    ConnectionTrait,
    DatabaseBackend,
    Statement,
};
use sha2::{
    Digest,
    Sha256,
};
use tempfile::TempDir;

use super::*;
use crate::analytics::{
    BUFFER_FILE,
    record::RequestRecord,
};

/// 北京时间 2026-09-27 0 点
const SEP_27: i64 = 1_790_438_400_000;
const HOUR: i64 = 3_600_000;

async fn setup() -> (TempDir, ConverterOptions) {
    let dir = tempfile::tempdir().unwrap();
    let db = buffer::open(&dir.path().join(BUFFER_FILE)).await.unwrap();
    let options = ConverterOptions {
        db,
        dir: dir.path().to_owned(),
        instance: Arc::from("3000@test"),
        retention: RetentionPolicy {
            retention_days: 90,
            max_total_bytes: u64::MAX,
        },
    };
    (dir, options)
}

fn record(ts: i64, route: &str) -> RequestRecord {
    RequestRecord {
        ts,
        method: "GET".to_string(),
        prefix: Some("/v1"),
        route: Some(route.to_string()),
        raw_path: None,
        params: Some(r#"{"q":"x"}"#.to_string()),
        status: 200,
        latency_us: 1234,
        resp_bytes: Some(100),
        conditional: false,
        user_agent: Some("AMLL Player/1.0".to_string()),
        origin: None,
        referer: None,
        client_id: Some(-42),
        instance: Arc::from("3000@test"),
        hit_count: Some(1),
        hit_id: Some(7),
        match_kind: None,
        norm_query: None,
    }
}

async fn insert(options: &ConverterOptions, records: &[RequestRecord]) {
    buffer::insert_batch(&options.db, records).await.unwrap();
}

async fn buffered_count(options: &ConverterOptions) -> i64 {
    options
        .db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM requests",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}

fn daily_path(dir: &TempDir, day: &str) -> std::path::PathBuf {
    dir.path().join(PARQUET_DIR).join(format!("{day}.parquet"))
}

fn read_parquet(path: &Path) -> (Vec<RecordBatch>, Option<String>) {
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(path).unwrap()).unwrap();
    let schema_version = builder
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .and_then(|kv| kv.iter().find(|kv| kv.key == "amll.schema_version"))
        .and_then(|kv| kv.value.clone());
    let batches = builder.build().unwrap().collect::<Result<_, _>>().unwrap();
    (batches, schema_version)
}

#[tokio::test]
async fn converts_closed_days_and_keeps_today() {
    let (dir, options) = setup().await;
    insert(
        &options,
        &[
            // 09-26 的三行故意乱序写入，文件里应按 ts 排好
            record(SEP_27 - HOUR, "/lyrics/get"),
            record(SEP_27 - 20 * HOUR, "/lyrics/search"),
            record(SEP_27 - 1, "/lrclib/get"),
            // 09-25 一行
            record(SEP_27 - 30 * HOUR, "/status"),
            // 09-27（今天）一行
            record(SEP_27 + HOUR, "/lyrics/list"),
        ],
    )
    .await;

    run_once(&options, SEP_27 + 12 * HOUR).await.unwrap();

    assert!(daily_path(&dir, "2026-09-25").exists());
    assert!(!daily_path(&dir, "2026-09-27").exists(), "今天还没收齐");
    assert_eq!(buffered_count(&options).await, 1);

    let path = daily_path(&dir, "2026-09-26");
    let (batches, schema_version) = read_parquet(&path);
    assert_eq!(schema_version.as_deref(), Some("1"));
    assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 3);

    let batch = &batches[0];
    let ts = batch
        .column_by_name("ts")
        .unwrap()
        .as_any()
        .downcast_ref::<TimestampMillisecondArray>()
        .unwrap();
    assert_eq!(
        ts.values().to_vec(),
        [SEP_27 - 20 * HOUR, SEP_27 - HOUR, SEP_27 - 1]
    );
    let routes = batch
        .column_by_name("route")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(routes.value(0), "/lyrics/search");
    assert!(batch.column_by_name("match_kind").unwrap().is_null(0));

    // 转换记录里的 sha256 与字节数对得上文件本身
    let row = options
            .db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT state, rows, bytes, sha256, min_ts, max_ts FROM conversions WHERE day = '2026-09-26'",
            ))
            .await
            .unwrap()
            .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(row.try_get::<String>("", "state").unwrap(), "done");
    assert_eq!(row.try_get::<i64>("", "rows").unwrap(), 3);
    assert_eq!(
        row.try_get::<i64>("", "bytes").unwrap(),
        i64::try_from(bytes.len()).unwrap()
    );
    assert_eq!(
        row.try_get::<String>("", "sha256").unwrap(),
        hex::encode(Sha256::digest(&bytes))
    );
    assert_eq!(
        row.try_get::<i64>("", "min_ts").unwrap(),
        SEP_27 - 20 * HOUR
    );
    assert_eq!(row.try_get::<i64>("", "max_ts").unwrap(), SEP_27 - 1);

    // 再跑一轮是空操作
    run_once(&options, SEP_27 + 12 * HOUR).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(buffered_count(&options).await, 1);
}

#[tokio::test]
async fn waits_for_the_grace_period_after_midnight() {
    let (dir, options) = setup().await;
    insert(&options, &[record(SEP_27 - HOUR, "/status")]).await;

    run_once(&options, SEP_27 + 5 * 60 * 1000).await.unwrap();
    assert!(!daily_path(&dir, "2026-09-26").exists());

    run_once(&options, SEP_27 + 11 * 60 * 1000).await.unwrap();
    assert!(daily_path(&dir, "2026-09-26").exists());
    assert_eq!(buffered_count(&options).await, 0);
}

#[tokio::test]
async fn respects_a_live_claim_and_takes_over_an_expired_one() {
    let (dir, options) = setup().await;
    insert(&options, &[record(SEP_27 - HOUR, "/status")]).await;
    let now = SEP_27 + 12 * HOUR;

    let claimed = buffer::claim_day(&options.db, "2026-09-26", "3001@other", now - 60_000, 0)
        .await
        .unwrap();
    assert!(matches!(claimed, Claim::Acquired { .. }));

    run_once(&options, now).await.unwrap();
    assert!(
        !daily_path(&dir, "2026-09-26").exists(),
        "另一个实例的认领仍有效"
    );
    assert_eq!(buffered_count(&options).await, 1);

    run_once(&options, now + LEASE_MS).await.unwrap();
    assert!(daily_path(&dir, "2026-09-26").exists(), "租约过期后接手");
    assert_eq!(buffered_count(&options).await, 0);
}

#[tokio::test]
async fn late_holder_cannot_finish_after_takeover() {
    let (_dir, options) = setup().await;
    let summary = buffer::DaySummary {
        rows: 0,
        bytes: 0,
        sha256: String::new(),
        min_ts: None,
        max_ts: None,
    };

    let Claim::Acquired { claimed_at: first } =
        buffer::claim_day(&options.db, "2026-09-26", "3000@a", 1_000, 0)
            .await
            .unwrap()
    else {
        panic!("first claim should succeed");
    };
    let Claim::Acquired { claimed_at: second } = buffer::claim_day(
        &options.db,
        "2026-09-26",
        "3001@b",
        1_000 + LEASE_MS + 1,
        1_001,
    )
    .await
    .unwrap() else {
        panic!("expired claim should be taken over");
    };

    assert!(
        !buffer::finish_day(&options.db, "2026-09-26", "3000@a", first, &summary, 0)
            .await
            .unwrap()
    );
    assert!(
        buffer::finish_day(&options.db, "2026-09-26", "3001@b", second, &summary, 0)
            .await
            .unwrap()
    );
    assert_eq!(
        buffer::claim_day(&options.db, "2026-09-26", "3000@a", i64::MAX, i64::MAX)
            .await
            .unwrap(),
        Claim::Done
    );
}

#[tokio::test]
async fn deletes_leftover_rows_of_a_converted_day() {
    let (dir, options) = setup().await;
    insert(&options, &[record(SEP_27 - HOUR, "/status")]).await;
    run_once(&options, SEP_27 + 12 * HOUR).await.unwrap();
    let original = std::fs::read(daily_path(&dir, "2026-09-26")).unwrap();

    insert(&options, &[record(SEP_27 - 2 * HOUR, "/status")]).await;
    run_once(&options, SEP_27 + 12 * HOUR).await.unwrap();

    assert_eq!(buffered_count(&options).await, 0);
    assert_eq!(
        std::fs::read(daily_path(&dir, "2026-09-26")).unwrap(),
        original,
        "已发布的文件不再改动"
    );
}

#[tokio::test]
async fn converts_a_multi_day_backlog_in_one_run() {
    let (dir, options) = setup().await;
    let records: Vec<_> = (1..=5)
        .map(|days_ago| record(SEP_27 - days_ago * DAY_MS + HOUR, "/status"))
        .collect();
    insert(&options, &records).await;

    run_once(&options, SEP_27 + 12 * HOUR).await.unwrap();

    for day in [
        "2026-09-22",
        "2026-09-23",
        "2026-09-24",
        "2026-09-25",
        "2026-09-26",
    ] {
        assert!(daily_path(&dir, day).exists(), "{day} 应已转换");
    }
    assert_eq!(buffered_count(&options).await, 0);
}
