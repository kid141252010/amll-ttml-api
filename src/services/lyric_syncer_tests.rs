use std::sync::{
    Arc,
    atomic::{
        AtomicBool,
        AtomicUsize,
        Ordering,
    },
};

use sea_orm::{
    ConnectionTrait,
    EntityTrait,
    IntoActiveModel,
};
use tokio::sync::Notify;

use super::*;
use crate::{
    core::db::{
        entity,
        setup::init_db,
    },
    services::sync_service::build_entity_from_ttml,
    utils::ttml::TTMLFormatResult,
};

const TTML: &str = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:02.000">hello</p></div></body></tt>"#;

fn result(status: SyncStatus) -> SyncResult {
    SyncResult {
        status,
        count: None,
        error: None,
        strategy: None,
    }
}

/// 假的同步步骤：不出网，记录调用次数与 `force`，返回固定状态
fn fake_syncer(
    store: &DbLyricStore,
    status: SyncStatus,
) -> (LyricSyncer, Arc<AtomicUsize>, Arc<AtomicBool>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let last_force = Arc::new(AtomicBool::new(false));
    let (c, f) = (calls.clone(), last_force.clone());
    let syncer = LyricSyncer::with_runner(store.clone(), move |force| {
        c.fetch_add(1, Ordering::SeqCst);
        f.store(force, Ordering::SeqCst);
        let status = status.clone();
        Box::pin(async move { Ok(result(status)) })
    });
    (syncer, calls, last_force)
}

/// 空索引的存储，SQLite 里有一行公库数据（相当于同步已写库、索引尚未重建）
async fn store_with_one_row() -> DbLyricStore {
    let conn = init_db("sqlite::memory:").await.expect("init db");
    let parsed = ttml_processor::parse_ttml(TTML).expect("sample ttml");
    let model = build_entity_from_ttml("one.ttml", TTML, &parsed);
    entity::Entity::insert(model.into_active_model())
        .exec(&conn)
        .await
        .expect("seed row");
    DbLyricStore::new(conn)
}

async fn warm_caches(store: &DbLyricStore) {
    store
        .ttml_cache
        .insert("cached.ttml".to_string(), "stale".to_string())
        .await;
    store
        .formatted_lyric_cache
        .insert("cached.ttml".to_string(), TTMLFormatResult::default())
        .await;
}

async fn caches_are_warm(store: &DbLyricStore) -> (bool, bool) {
    (
        store.ttml_cache.get("cached.ttml").await.is_some(),
        store
            .formatted_lyric_cache
            .get("cached.ttml")
            .await
            .is_some(),
    )
}

#[tokio::test]
async fn updated_rebuilds_index_and_clears_caches() {
    let store = store_with_one_row().await;
    warm_caches(&store).await;
    let (syncer, calls, last_force) = fake_syncer(&store, SyncStatus::Updated);

    syncer.sync(true).await.expect("sync");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(last_force.load(Ordering::SeqCst), "force is passed through");
    assert_eq!(store.lyric_count(), 1);
    assert_eq!(caches_are_warm(&store).await, (false, false));
}

#[tokio::test]
async fn skipped_and_empty_keep_index_and_caches() {
    for status in [SyncStatus::Skipped, SyncStatus::Empty] {
        let store = store_with_one_row().await;
        warm_caches(&store).await;
        let (syncer, calls, last_force) = fake_syncer(&store, status.clone());

        syncer.sync(false).await.expect("sync");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!last_force.load(Ordering::SeqCst));
        // 库里有行但没重建：索引仍是旧的（空）
        assert_eq!(store.lyric_count(), 0, "{status:?}");
        assert_eq!(caches_are_warm(&store).await, (true, true), "{status:?}");
    }
}

#[tokio::test]
async fn rebuild_failure_keeps_previous_index_and_caches() {
    let store = store_with_one_row().await;
    store.rebuild_index().await.expect("initial rebuild");
    let previous = store.index.load_full();
    warm_caches(&store).await;

    // 元数据列损坏：同步「成功」但重建失败
    store
        .conn
        .execute_unprepared("UPDATE lyrics SET track_names = 'invalid json'")
        .await
        .expect("corrupt metadata");
    let (syncer, _, _) = fake_syncer(&store, SyncStatus::Updated);

    syncer
        .sync(false)
        .await
        .expect("rebuild failure is not an error");

    assert!(Arc::ptr_eq(&previous, &store.index.load_full()));
    assert_eq!(caches_are_warm(&store).await, (true, true));
}

#[tokio::test]
async fn sync_failure_maps_to_upstream_error_and_touches_nothing() {
    let store = store_with_one_row().await;
    warm_caches(&store).await;
    let syncer = LyricSyncer::with_runner(store.clone(), |_| {
        Box::pin(async { Err(anyhow::anyhow!("index download failed")) })
    });

    let err = syncer.sync(false).await.unwrap_err();

    assert!(
        matches!(err, AppError::UpstreamError(ref msg) if msg.contains("index download failed"))
    );
    assert_eq!(store.lyric_count(), 0);
    assert_eq!(caches_are_warm(&store).await, (true, true));
}

/// 同步在途时再次触发（含克隆出来的句柄）直接跳过，不排队；结束后锁释放
#[tokio::test]
async fn concurrent_triggers_are_single_flight() {
    let store = store_with_one_row().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let syncer = {
        let (calls, started, release) = (calls.clone(), started.clone(), release.clone());
        LyricSyncer::with_runner(store.clone(), move |_| {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            let (started, release) = (started.clone(), release.clone());
            Box::pin(async move {
                // 只有第一次调用阻塞，用来制造「在途」窗口
                if n == 0 {
                    started.notify_one();
                    release.notified().await;
                }
                Ok(result(SyncStatus::Skipped))
            })
        })
    };

    let in_flight = tokio::spawn({
        let syncer = syncer.clone();
        async move { syncer.sync(false).await }
    });
    started.notified().await;

    syncer.sync(true).await.expect("duplicate trigger");
    syncer.clone().sync(false).await.expect("duplicate trigger");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "duplicates are skipped");

    release.notify_one();
    in_flight.await.expect("join").expect("first sync");

    syncer.sync(false).await.expect("next sync");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "lock released afterwards");
}
