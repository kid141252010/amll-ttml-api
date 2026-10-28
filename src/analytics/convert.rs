//! 每日转换：北京时间自然日收齐后，把缓冲库里这一天的行转成 Parquet，再从缓冲库删掉
//!
//! 每 5 分钟检查一次。过了 0 点 10 分才处理前一天，给还在写入任务缓冲里的数据留出宽限；
//! 缓冲里积压的多个整天（例如这个功能上线前积攒的）会在一轮里依次补转。
//!
//! 蓝绿两个实例都跑这个任务，靠 `conversions` 表的认领保证同一天只转换一次：
//! - 先写临时文件再 rename 到位，然后标记完成，最后按小批删除原始行
//! - 任何一步崩溃都能恢复：rename 之前崩溃，认领租约过期后重新转换并覆盖文件；
//!   标记完成之后崩溃，残留的原始行在下一轮被当作「已转换日期的残留」删掉
//!
//! 每轮转换之后执行保留策略

use std::{
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use chrono::NaiveDate;
use sea_orm::DatabaseConnection;
use tokio::{
    task::JoinHandle,
    time::MissedTickBehavior,
};
use tracing::{
    error,
    info,
    warn,
};

use super::{
    buffer::{
        self,
        Claim,
    },
    day::{
        DAY_MS,
        day_of,
        day_start_ms,
        file_name,
    },
    parquet_file::DailyFileWriter,
    record::now_millis,
    retention::{
        self,
        RetentionPolicy,
    },
};

/// 每日文件在统计目录下的子目录
pub const PARQUET_DIR: &str = "parquet";

const CONVERT_INTERVAL: Duration = Duration::from_mins(5);
/// 过了 0 点多久才转换前一天
const GRACE_MS: i64 = 10 * 60 * 1000;
/// 认领的租约。正常转换一天只要几秒，超过这个时长仍未完成视为持有者已经崩溃
const LEASE_MS: i64 = 30 * 60 * 1000;
/// 每次从缓冲库读取的行数，给内存封顶
const READ_CHUNK_ROWS: u64 = 10_000;

pub struct ConverterOptions {
    /// 转换任务专用的连接，长时间的读取不占写入任务的连接
    pub db: DatabaseConnection,
    /// 统计目录
    pub dir: PathBuf,
    pub instance: Arc<str>,
    pub retention: RetentionPolicy,
}

pub fn spawn(options: ConverterOptions) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(CONVERT_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut failing = false;

        loop {
            ticker.tick().await;

            // 只在状态切换时打日志，持续故障时不至于每 5 分钟往 Sentry 报一条
            match run_once(&options, now_millis()).await {
                Ok(()) => {
                    if failing {
                        failing = false;
                        info!("Request analytics conversion recovered");
                    }
                }
                Err(e) => {
                    if !failing {
                        failing = true;
                        error!("Request analytics conversion failed, will retry: {e:?}");
                    }
                }
            }
        }
    })
}

/// 转换所有已收齐的整天，然后执行保留策略
pub async fn run_once(options: &ConverterOptions, now: i64) -> anyhow::Result<()> {
    convert_closed_days(options, now).await?;

    let today = day_of(now).context("Current time is out of range")?;
    let dir = options.dir.clone();
    let policy = options.retention;
    tokio::task::spawn_blocking(move || {
        retention::apply(&dir, &dir.join(PARQUET_DIR), today, &policy)
    })
    .await?
    .context("Failed to apply analytics retention")
}

async fn convert_closed_days(options: &ConverterOptions, now: i64) -> anyhow::Result<()> {
    // 截止点是「往前推宽限期后所在北京日」的 0 点，早于它的整天都已收齐
    let cutoff = day_of(now - GRACE_MS)
        .map(day_start_ms)
        .context("Current time is out of range")?;

    while let Some(oldest) = buffer::oldest_ts_before(&options.db, cutoff).await? {
        let day = day_of(oldest).context("Buffered timestamp is out of range")?;
        let day_key = day.format("%Y-%m-%d").to_string();
        let start = day_start_ms(day);

        let claim = buffer::claim_day(
            &options.db,
            &day_key,
            &options.instance,
            now,
            now - LEASE_MS,
        )
        .await?;

        match claim {
            Claim::Acquired { claimed_at } => {
                convert_day(options, day, &day_key, claimed_at).await?;
            }
            Claim::Done => {
                let deleted = buffer::delete_range(&options.db, start, start + DAY_MS).await?;
                warn!(
                    day = %day_key,
                    deleted,
                    "Deleted leftover buffered rows of an already converted day"
                );
            }
            // 另一个实例正在转换最旧的这一天，下一轮再看
            Claim::Held => break,
        }
    }

    Ok(())
}

async fn convert_day(
    options: &ConverterOptions,
    day: NaiveDate,
    day_key: &str,
    claimed_at: i64,
) -> anyhow::Result<()> {
    let start = day_start_ms(day);
    let end = start + DAY_MS;

    let parquet_dir = options.dir.join(PARQUET_DIR);
    tokio::fs::create_dir_all(&parquet_dir).await?;

    let mut writer =
        DailyFileWriter::create(&parquet_dir, &file_name(day), day_key, &options.instance)?;
    let mut after = None;

    loop {
        let rows = match buffer::read_range(&options.db, start, end, after, READ_CHUNK_ROWS).await {
            Ok(rows) => rows,
            Err(e) => {
                writer.abort();
                return Err(e.into());
            }
        };

        let Some(last) = rows.last() else {
            break;
        };
        after = Some((last.ts, last.row_id));
        writer = writer.write(rows).await?;
    }

    let summary = writer.finish().await?;

    let still_held = buffer::finish_day(
        &options.db,
        day_key,
        &options.instance,
        claimed_at,
        &summary,
        now_millis(),
    )
    .await?;
    if !still_held {
        // 租约过期后已被另一个实例接手，由它负责标记完成与删除原始行
        warn!(day = %day_key, "Lost the analytics conversion claim before finishing");
        return Ok(());
    }

    let deleted = buffer::delete_range(&options.db, start, end).await?;
    info!(
        day = %day_key,
        rows = summary.rows,
        bytes = summary.bytes,
        deleted,
        "Converted request analytics day to Parquet"
    );

    Ok(())
}

#[cfg(test)]
mod tests;
