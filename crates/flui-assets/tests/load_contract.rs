//! Validation gates registry admission, including cache hits.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_assets::{Asset, AssetError, AssetKey, AssetRegistryBuilder, FontAsset};

struct OwnedValue {
    generation: usize,
    drops: Arc<AtomicUsize>,
}

impl Drop for OwnedValue {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct OwnedAsset {
    loads: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

struct ReentrantValue {
    registry: std::sync::Weak<flui_assets::AssetRegistry>,
    observed: Arc<AtomicUsize>,
}

impl Drop for ReentrantValue {
    fn drop(&mut self) {
        use std::future::Future;
        use std::task::{Context, Poll, Waker};

        let observation = if let Some(registry) = self.registry.upgrade() {
            let key = "reentrant".to_owned();
            let mut lookup = std::pin::pin!(registry.get::<ReentrantAsset>(&key));
            match lookup
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            {
                Poll::Ready(None) => 1,
                Poll::Ready(Some(_)) | Poll::Pending => 3,
            }
        } else {
            2
        };
        self.observed.store(observation, Ordering::Relaxed);
    }
}

struct ReentrantAsset {
    registry: std::sync::Weak<flui_assets::AssetRegistry>,
    observed: Arc<AtomicUsize>,
}

impl Asset for ReentrantAsset {
    type Data = ReentrantValue;
    type Key = String;
    type Error = AssetError;

    fn key(&self) -> String {
        "reentrant".into()
    }

    async fn load(&self) -> Result<ReentrantValue, AssetError> {
        Ok(ReentrantValue {
            registry: self.registry.clone(),
            observed: Arc::clone(&self.observed),
        })
    }
}

fn retirement_commits_before_reentry(release_owner: bool) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let observation = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("retirement runtime starts")
            .block_on(async {
                let registry =
                    Arc::new(AssetRegistryBuilder::new().with_default_capacity().build());
                let observed = Arc::new(AtomicUsize::new(0));
                let data = registry
                    .load(ReentrantAsset {
                        registry: Arc::downgrade(&registry),
                        observed: Arc::clone(&observed),
                    })
                    .await
                    .expect("reentrant data loads");
                drop(data);
                if release_owner {
                    drop(registry);
                } else {
                    registry.clear_all().await;
                    let fresh = registry
                        .load(CheckedAsset {
                            accepted: true,
                            loads: Arc::new(AtomicUsize::new(0)),
                        })
                        .await
                        .expect("fresh cache works after retirement");
                    assert_eq!(*fresh, 42);
                }
                observed.load(Ordering::Relaxed)
            });
        sender
            .send(observation)
            .expect("retirement result is delivered");
    });
    let observed = receiver
        .recv_timeout(std::time::Duration::from_secs(3))
        .expect("cache retirement permits destructor reentry before the deadline");
    worker.join().expect("retirement worker completes");
    assert_eq!(observed, if release_owner { 2 } else { 1 });
}

fn clearing_detaches_the_map_before_destructor_reentry() {
    retirement_commits_before_reentry(false);
}

fn releasing_the_last_owner_has_an_absent_owner_fallback() {
    retirement_commits_before_reentry(true);
}

#[test]
fn registry_cache_retirement_commits_ownership_before_data_drop() {
    crate::cases::run_cases(&[
        (
            "clear permits reentry",
            clearing_detaches_the_map_before_destructor_reentry,
        ),
        (
            "last owner is absent",
            releasing_the_last_owner_has_an_absent_owner_fallback,
        ),
    ]);
}

impl Asset for OwnedAsset {
    type Data = OwnedValue;
    type Key = String;
    type Error = AssetError;

    fn key(&self) -> String {
        "owned".into()
    }

    async fn load(&self) -> Result<OwnedValue, AssetError> {
        Ok(OwnedValue {
            generation: self.loads.fetch_add(1, Ordering::Relaxed) + 1,
            drops: Arc::clone(&self.drops),
        })
    }
}

#[tokio::test]
async fn non_clone_data_retains_evicted_handles_across_reload() {
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let loads = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let asset = || OwnedAsset {
        loads: Arc::clone(&loads),
        drops: Arc::clone(&drops),
    };
    let key = "owned".to_owned();
    let first = registry.load(asset()).await.expect("non-Clone data loads");
    let cloned = first.clone();
    let weak = first.downgrade();
    let weak_clone = weak.clone();
    let cached = registry
        .load(asset())
        .await
        .expect("cache hit shares the value");
    let fetched = registry
        .get::<OwnedAsset>(&key)
        .await
        .expect("cached data is retrievable");
    registry
        .preload(asset())
        .await
        .expect("preload accepts non-Clone data");
    assert_eq!(loads.load(Ordering::Relaxed), 1);
    assert!(first.ptr_eq(&cloned));
    assert!(first.ptr_eq(&cached));
    assert!(first.ptr_eq(&fetched));

    registry.invalidate::<OwnedAsset>(&key).await;
    registry.clear::<OwnedAsset>().await;
    assert!(registry.get::<OwnedAsset>(&key).await.is_none());
    assert_eq!(first.generation, 1, "held handles outlive cache eviction");
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert!(weak_clone.upgrade().is_some());

    let reloaded = registry
        .load(asset())
        .await
        .expect("evicted non-Clone data reloads while old handles remain alive");
    assert_eq!(first.key(), reloaded.key());
    assert!(first.ptr_eq(&cloned));
    assert!(
        !first.ptr_eq(&reloaded),
        "equal keys do not imply shared ownership"
    );
    assert_eq!(reloaded.generation, 2);
    assert_eq!(loads.load(Ordering::Relaxed), 2);
    drop((first, cloned, cached, fetched));
    // Invalidation excludes lookups; Moka may still own retired data until
    // deferred maintenance completes. It is not a physical release barrier.
    let reloaded_weak = reloaded.downgrade();
    drop(reloaded);
    assert!(
        reloaded_weak.upgrade().is_some(),
        "the populated cache owns the new generation"
    );
    drop(registry);
    assert!(weak.upgrade().is_none());
    assert!(weak_clone.upgrade().is_none());
    assert!(reloaded_weak.upgrade().is_none());
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}

struct CheckedAsset {
    accepted: bool,
    loads: Arc<AtomicUsize>,
}

#[test]
fn font_sources_preserve_bytes_and_recover_after_load_errors() {
    crate::cases::run_cases(&[
        ("short font", short_font_recovers),
        ("invalid magic", invalid_font_recovers),
    ]);
}

fn short_font_recovers() {
    font_sources_recover(Vec::new());
}

fn invalid_font_recovers() {
    font_sources_recover(b"invalid font".to_vec());
}

fn font_sources_recover(invalid: Vec<u8>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the font test runtime starts")
        .block_on(font_source_sequence(invalid));
}

async fn font_source_sequence(invalid: Vec<u8>) {
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let font_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../flui-painting/assets/fonts/probe-sans-400.ttf"
    );
    let bytes = include_bytes!("../../flui-painting/assets/fonts/probe-sans-400.ttf");
    let embedded_key = "embedded-font";

    assert!(
        registry
            .load(FontAsset::from_bytes(embedded_key, invalid))
            .await
            .is_err()
    );
    assert!(
        registry
            .get::<FontAsset>(&AssetKey::new(embedded_key))
            .await
            .is_none()
    );
    let embedded = registry
        .load(FontAsset::from_bytes(embedded_key, bytes.to_vec()))
        .await
        .expect("valid font bytes recover after invalid source data");
    assert_eq!(embedded.bytes.as_slice(), bytes);

    let missing = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/does-not-exist.ttf"
    );
    assert!(registry.load(FontAsset::file(missing)).await.is_err());
    let file = registry
        .load(FontAsset::file(font_path))
        .await
        .expect("file loading recovers after a missing source");
    assert_eq!(file.bytes.as_slice(), bytes);
}

impl Asset for CheckedAsset {
    type Data = u32;
    type Key = String;
    type Error = AssetError;

    fn key(&self) -> String {
        "checked".into()
    }

    fn validate(&self) -> Result<(), AssetError> {
        if self.accepted {
            Ok(())
        } else {
            Err(AssetError::InvalidData {
                path: "checked".into(),
                reason: "descriptor rejected".into(),
            })
        }
    }

    async fn load(&self) -> Result<u32, AssetError> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        Ok(42)
    }
}

#[tokio::test]
async fn validation_precedes_loading_and_cache_hits_without_poisoning_accepted_data() {
    let registry = AssetRegistryBuilder::new().with_default_capacity().build();
    let loads = Arc::new(AtomicUsize::new(0));
    let asset = |accepted| CheckedAsset {
        accepted,
        loads: Arc::clone(&loads),
    };
    let key = "checked".to_owned();

    assert!(matches!(
        registry.load(asset(false)).await,
        Err(AssetError::InvalidData { .. })
    ));
    assert_eq!(loads.load(Ordering::Relaxed), 0);
    assert!(registry.get::<CheckedAsset>(&key).await.is_none());

    let accepted = registry
        .load(asset(true))
        .await
        .expect("accepted descriptor loads");
    assert_eq!(*accepted, 42);
    assert!(matches!(
        registry.load(asset(false)).await,
        Err(AssetError::InvalidData { .. })
    ));
    assert_eq!(loads.load(Ordering::Relaxed), 1);
    let cached = registry
        .get::<CheckedAsset>(&key)
        .await
        .expect("rejection preserves accepted data");
    assert_eq!(*cached, 42);

    let recovered = registry
        .load(asset(true))
        .await
        .expect("a later accepted request succeeds");
    assert_eq!(*recovered, 42);
    assert_eq!(
        loads.load(Ordering::Relaxed),
        1,
        "accepted cache hits do not reload"
    );
}

struct GatedAsset {
    loads: Arc<AtomicUsize>,
    gate: Arc<tokio::sync::Semaphore>,
    fail: bool,
}

impl Asset for GatedAsset {
    type Data = usize;
    type Key = String;
    type Error = AssetError;

    fn key(&self) -> String {
        "gated".into()
    }

    async fn load(&self) -> Result<usize, AssetError> {
        let generation = self.loads.fetch_add(1, Ordering::Relaxed) + 1;
        self.gate
            .acquire()
            .await
            .expect("gate remains open")
            .forget();
        if self.fail {
            Err(AssetError::LoadFailed {
                path: "gated".into(),
                reason: format!("attempt {generation}"),
            })
        } else {
            Ok(generation)
        }
    }
}

fn run_cold_load_case(case: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the load test runtime starts")
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), case)
                .await
                .expect("cold load waiters must make progress");
        });
}

async fn poll_until_loading<F: std::future::Future>(
    mut future: std::pin::Pin<&mut F>,
    loads: &AtomicUsize,
    count: usize,
) {
    std::future::poll_fn(|cx| {
        assert!(
            future.as_mut().poll(cx).is_pending(),
            "gate must hold the load"
        );
        if loads.load(Ordering::Relaxed) >= count {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
}

async fn poll_waiter<F: std::future::Future>(mut future: std::pin::Pin<&mut F>) {
    std::future::poll_fn(|cx| {
        assert!(
            future.as_mut().poll(cx).is_pending(),
            "waiter must await the held load"
        );
        std::task::Poll::Ready(())
    })
    .await;
}

fn concurrent_cold_success_shares_the_loaded_allocation() {
    run_cold_load_case(async {
        let registry = AssetRegistryBuilder::new().with_default_capacity().build();
        let loads = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let asset = || GatedAsset {
            loads: Arc::clone(&loads),
            gate: Arc::clone(&gate),
            fail: false,
        };
        let mut first = Box::pin(registry.load(asset()));
        let mut second = Box::pin(registry.load(asset()));
        poll_until_loading(first.as_mut(), &loads, 1).await;
        poll_waiter(second.as_mut()).await;
        assert_eq!(
            loads.load(Ordering::Relaxed),
            1,
            "cold callers share loader work"
        );
        gate.add_permits(2);
        let first = first.await.expect("first load succeeds");
        let second = second.await.expect("waiting load succeeds");
        assert!(
            first.ptr_eq(&second),
            "both callers retain one decoded allocation"
        );
        let cached = registry.load(asset()).await.expect("cached load succeeds");
        assert!(first.ptr_eq(&cached));
        assert_eq!(loads.load(Ordering::Relaxed), 1);
    });
}

fn concurrent_cold_error_is_shared_and_a_later_request_retries() {
    run_cold_load_case(async {
        let registry = AssetRegistryBuilder::new().with_default_capacity().build();
        let loads = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let asset = |fail| GatedAsset {
            loads: Arc::clone(&loads),
            gate: Arc::clone(&gate),
            fail,
        };
        let mut first = Box::pin(registry.load(asset(true)));
        let mut second = Box::pin(registry.load(asset(true)));
        poll_until_loading(first.as_mut(), &loads, 1).await;
        poll_waiter(second.as_mut()).await;
        gate.add_permits(3);
        let first = first.await.expect_err("first load fails");
        let second = second.await.expect_err("waiting load fails");
        assert_eq!(
            first.to_string(),
            second.to_string(),
            "waiters observe one failure"
        );
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        let next = registry
            .load(asset(false))
            .await
            .expect("error is not cached");
        assert_eq!(*next, 2, "the next accepted descriptor retries loading");
    });
}

fn canceling_the_initializer_restarts_a_waiting_load() {
    run_cold_load_case(async {
        let registry = AssetRegistryBuilder::new().with_default_capacity().build();
        let loads = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let asset = || GatedAsset {
            loads: Arc::clone(&loads),
            gate: Arc::clone(&gate),
            fail: false,
        };
        let mut first = Box::pin(registry.load(asset()));
        let mut second = Box::pin(registry.load(asset()));
        poll_until_loading(first.as_mut(), &loads, 1).await;
        poll_waiter(second.as_mut()).await;
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        drop(first);
        poll_until_loading(second.as_mut(), &loads, 2).await;
        gate.add_permits(1);
        let second = second.await.expect("waiter initializes after cancellation");
        assert_eq!(*second, 2);
        let cached = registry
            .load(asset())
            .await
            .expect("completed retry is cached");
        assert!(second.ptr_eq(&cached));
        assert_eq!(loads.load(Ordering::Relaxed), 2);
    });
}

#[test]
fn cold_registry_loads_share_work_and_recover() {
    crate::cases::run_cases(&[
        (
            "concurrent cold success shares the loaded allocation",
            concurrent_cold_success_shares_the_loaded_allocation,
        ),
        (
            "concurrent cold error is shared and a later request retries",
            concurrent_cold_error_is_shared_and_a_later_request_retries,
        ),
        (
            "canceling the initializer restarts a waiting load",
            canceling_the_initializer_restarts_a_waiting_load,
        ),
        (
            "public cache waiters share data and non-Clone errors",
            public_cache_waiters_share_data_and_non_clone_errors,
        ),
        (
            "public cache waiters recover after cancellation or panic",
            public_cache_waiters_recover_after_cancellation_or_panic,
        ),
    ]);
}

/// Cache clones account for operations on the same entries, including reset.
#[tokio::test]
async fn cache_clones_report_shared_operations_and_reset() {
    use flui_assets::AssetCache;

    let cache = AssetCache::<GatedAsset>::with_config(flui_assets::AssetCacheConfig {
        capacity: flui_assets::CacheCapacity::Entries(
            std::num::NonZeroU64::new(100).expect("nonzero test capacity"),
        ),
        time_to_live: flui_assets::CacheExpiration::after(std::time::Duration::from_mins(1))
            .expect("supported test expiration"),
        ..flui_assets::AssetCacheConfig::default()
    });
    let key = "shared".to_owned();
    cache.insert(key.clone(), 7).await;
    let observer = cache.clone();
    assert_eq!(
        *observer
            .get(&key)
            .await
            .expect("clones share cached entries"),
        7
    );
    assert!(cache.get(&"missing".to_owned()).await.is_none());
    let counts = |stats: flui_assets::cache::CacheStats| {
        (
            stats.hits,
            stats.misses,
            stats.insertions,
            stats.invalidations,
        )
    };
    assert_eq!(counts(cache.stats()), (1, 1, 1, 0));
    assert_eq!(counts(observer.stats()), (1, 1, 1, 0));
    assert!(cache.contains(&key));
    assert!(!observer.contains(&"absent".to_owned()));
    assert_eq!(
        counts(cache.stats()),
        (1, 1, 1, 0),
        "presence is observational"
    );

    observer.invalidate(&key).await;
    assert!(
        cache.get(&key).await.is_none(),
        "invalidation affects the shared entry"
    );
    assert_eq!(counts(cache.stats()), (1, 2, 1, 1));
    assert_eq!(counts(observer.stats()), (1, 2, 1, 1));
    observer.invalidate(&"absent".to_owned()).await;
    assert_eq!(
        counts(cache.stats()),
        (1, 2, 1, 2),
        "invalidation counts requests"
    );
    cache.insert(key.clone(), 9).await;
    observer.clear().await;
    assert_eq!(
        counts(cache.stats()),
        (0, 0, 0, 0),
        "clear resets shared counters"
    );
    assert!(
        cache.get(&key).await.is_none(),
        "clear removes the shared entry"
    );
    observer.reset_stats();
    assert_eq!(counts(cache.stats()), (0, 0, 0, 0));
}

#[derive(Debug, thiserror::Error)]
#[error("attempt {0}")]
struct NonCloneLoadError(usize);

struct PublicCacheAsset;

impl Asset for PublicCacheAsset {
    type Data = usize;
    type Key = String;
    type Error = NonCloneLoadError;

    fn key(&self) -> String {
        "public".to_owned()
    }

    async fn load(&self) -> Result<usize, NonCloneLoadError> {
        Ok(1)
    }
}

fn public_cache_waiters_share_data_and_non_clone_errors() {
    run_cold_load_case(async {
        for fail in [false, true] {
            let cache = flui_assets::AssetCache::<PublicCacheAsset>::new(
                flui_assets::CacheCapacity::default(),
            );
            let observer = cache.clone();
            let key = "shared".to_owned();
            let loads = Arc::new(AtomicUsize::new(0));
            let gate = Arc::new(tokio::sync::Semaphore::new(0));
            let initialize = || async {
                let attempt = loads.fetch_add(1, Ordering::Relaxed) + 1;
                gate.acquire().await.expect("gate remains open").forget();
                if fail {
                    Err(NonCloneLoadError(attempt))
                } else {
                    Ok(attempt)
                }
            };
            let mut first = Box::pin(cache.get_or_insert_with(key.clone(), initialize));
            let mut second = Box::pin(observer.get_or_insert_with(key.clone(), initialize));
            poll_until_loading(first.as_mut(), &loads, 1).await;
            poll_waiter(second.as_mut()).await;
            assert_eq!(
                loads.load(Ordering::Relaxed),
                1,
                "public helper coalesces work"
            );
            gate.add_permits(2);
            let first = first.await;
            let second = second.await;
            if fail {
                let first = first.expect_err("initializer fails");
                let second = second.expect_err("waiter shares failure");
                assert!(
                    Arc::ptr_eq(&first, &second),
                    "non-Clone errors share ownership"
                );
                let recovered = cache
                    .get_or_insert_with(key.clone(), || async {
                        Ok(loads.fetch_add(1, Ordering::Relaxed) + 1)
                    })
                    .await
                    .expect("failed initialization is retryable");
                assert_eq!(*recovered, 2);
            } else {
                let first = first.expect("initializer succeeds");
                let second = second.expect("waiter succeeds");
                assert!(first.ptr_eq(&second), "one loaded allocation is returned");
                let cached = cache
                    .get_or_insert_with(key, || async { Ok(99) })
                    .await
                    .expect("completed value is reused");
                assert!(first.ptr_eq(&cached));
                assert_eq!(loads.load(Ordering::Relaxed), 1);
            }
            assert_eq!(cache.stats().insertions, 1, "one completed fresh result");
        }
    });
}

fn public_cache_waiters_recover_after_cancellation_or_panic() {
    run_cold_load_case(async {
        for panic_first in [false, true] {
            let cache = flui_assets::AssetCache::<PublicCacheAsset>::new(
                flui_assets::CacheCapacity::default(),
            );
            let key = "recover".to_owned();
            let loads = Arc::new(AtomicUsize::new(0));
            let gate = Arc::new(tokio::sync::Semaphore::new(0));
            let mut first = Box::pin(cache.get_or_insert_with(key.clone(), || async {
                let attempt = loads.fetch_add(1, Ordering::Relaxed) + 1;
                gate.acquire().await.expect("gate remains open").forget();
                assert!(!panic_first, "initializer panic");
                Ok(attempt)
            }));
            let mut second = Box::pin(cache.get_or_insert_with(key.clone(), || async {
                let attempt = loads.fetch_add(1, Ordering::Relaxed) + 1;
                gate.acquire().await.expect("gate remains open").forget();
                if panic_first {
                    Err(NonCloneLoadError(attempt))
                } else {
                    Ok(attempt)
                }
            }));
            poll_until_loading(first.as_mut(), &loads, 1).await;
            poll_waiter(second.as_mut()).await;
            assert_eq!(loads.load(Ordering::Relaxed), 1);
            if panic_first {
                gate.add_permits(1);
                std::future::poll_fn(|cx| {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        first.as_mut().poll(cx)
                    }));
                    match outcome {
                        Err(payload) => {
                            assert_eq!(payload.downcast_ref::<&str>(), Some(&"initializer panic"));
                            std::task::Poll::Ready(())
                        }
                        Ok(std::task::Poll::Pending) => std::task::Poll::Pending,
                        Ok(std::task::Poll::Ready(_)) => panic!("initializer must panic"),
                    }
                })
                .await;
            }
            drop(first);
            poll_until_loading(second.as_mut(), &loads, 2).await;
            gate.add_permits(1);
            let second = second.await;
            if panic_first {
                assert_eq!(
                    second
                        .expect_err("restarted waiter can fail independently")
                        .0,
                    2
                );
                let next = cache
                    .get_or_insert_with(key, || async { Ok(3) })
                    .await
                    .expect("next request recovers after both failures");
                assert_eq!(*next, 3);
            } else {
                assert_eq!(*second.expect("waiter restarts after cancellation"), 2);
            }
        }
    });
}

fn count_capacity_bounds_entries_and_preserves_consumer_handles() {
    run_cold_load_case(async {
        use flui_assets::{AssetCache, CacheCapacity};
        let cache = AssetCache::<PublicCacheAsset>::new(CacheCapacity::Entries(
            std::num::NonZeroU64::new(2).expect("nonzero test capacity"),
        ));
        let retained = cache.insert("a".into(), 1).await;
        cache.insert("b".into(), 2).await;
        cache.insert("c".into(), 3).await;
        cache.sync().await;
        assert!(
            cache.len() <= 2,
            "configured count bound applies after maintenance"
        );
        assert_eq!(*retained, 1, "retirement does not invalidate consumer data");
    });
}

fn disabled_retention_coalesces_then_reloads() {
    run_cold_load_case(async {
        use flui_assets::{AssetCache, CacheCapacity};
        let cache = AssetCache::<PublicCacheAsset>::new(CacheCapacity::Disabled);
        let key = "disabled".to_owned();
        let loads = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let initialize = || async {
            let attempt = loads.fetch_add(1, Ordering::Relaxed) + 1;
            gate.acquire().await.expect("gate remains open").forget();
            Ok(attempt)
        };
        let mut first = Box::pin(cache.get_or_insert_with(key.clone(), initialize));
        let mut second = Box::pin(cache.get_or_insert_with(key.clone(), initialize));
        poll_until_loading(first.as_mut(), &loads, 1).await;
        poll_waiter(second.as_mut()).await;
        gate.add_permits(2);
        let first = first.await.expect("initializer succeeds without retention");
        let second = second.await.expect("waiter shares accepted work");
        assert!(first.ptr_eq(&second));
        assert!(!cache.contains(&key));
        let next = cache
            .get_or_insert_with(key, || async {
                Ok(loads.fetch_add(1, Ordering::Relaxed) + 1)
            })
            .await
            .expect("later request reloads");
        assert_eq!(*next, 2);
        assert!(!first.ptr_eq(&next));
        cache.sync().await;
        assert_eq!(cache.len(), 0);
    });
}

fn utilization_reports_the_configured_capacity_and_rates_remain_finite() {
    run_cold_load_case(async {
        use flui_assets::{AssetCache, AssetCacheExt, CacheCapacity};
        let cache = AssetCache::<PublicCacheAsset>::new(CacheCapacity::Entries(
            std::num::NonZeroU64::new(4).expect("nonzero test capacity"),
        ));
        cache.insert("a".into(), 1).await;
        cache.insert("b".into(), 2).await;
        cache.sync().await;
        assert_eq!(
            cache.utilization(),
            0.5,
            "two entries occupy half of four slots"
        );
        for _ in 0..8 {
            cache
                .get(&"a".to_owned())
                .await
                .expect("retrieving a hot entry");
        }
        assert_eq!(
            cache.utilization(),
            0.5,
            "historical requests are not capacity"
        );
        let disabled = AssetCache::<PublicCacheAsset>::new(CacheCapacity::Disabled);
        disabled.insert("a".into(), 1).await;
        assert_eq!(disabled.utilization(), 0.0);

        let large = flui_assets::cache::CacheStats {
            hits: usize::MAX,
            misses: usize::MAX,
            ..Default::default()
        };
        assert_eq!(large.hit_rate(), 0.5);
        assert_eq!(large.miss_rate(), 0.5);
        assert_eq!(large.total_requests(), usize::MAX);
        assert_eq!(flui_assets::cache::CacheStats::default().miss_rate(), 0.0);
    });
}

fn expiration_is_checked_before_cache_construction() {
    run_cold_load_case(async {
        use flui_assets::{AssetCache, AssetCacheConfig, CacheExpiration};
        assert!(CacheExpiration::after(std::time::Duration::MAX).is_err());
        let max = CacheExpiration::after(std::time::Duration::from_hours(1_000 * 365 * 24))
            .expect("maximum supported expiration is admitted");
        for expiration in [CacheExpiration::NEVER, max] {
            let cache = AssetCache::<PublicCacheAsset>::with_config(AssetCacheConfig {
                time_to_live: expiration,
                time_to_idle: expiration,
                ..Default::default()
            });
            cache.insert("a".into(), 7).await;
            assert_eq!(
                *cache
                    .get(&"a".to_owned())
                    .await
                    .expect("supported config loads"),
                7
            );
        }
        let immediate = AssetCache::<PublicCacheAsset>::with_config(AssetCacheConfig {
            time_to_idle: CacheExpiration::after(std::time::Duration::ZERO)
                .expect("immediate expiration is supported"),
            ..Default::default()
        });
        let held = immediate.insert("a".into(), 9).await;
        assert!(
            !immediate.contains(&"a".to_owned()),
            "configured idle expiration applies"
        );
        assert_eq!(*held, 9, "expiration preserves consumer ownership");
    });
}

#[test]
fn asset_cache_retention_and_observation_contracts() {
    crate::cases::run_cases(&[
        (
            "count bound preserves consumer handles",
            count_capacity_bounds_entries_and_preserves_consumer_handles,
        ),
        (
            "disabled retention coalesces then reloads",
            disabled_retention_coalesces_then_reloads,
        ),
        (
            "utilization and finite counter arithmetic",
            utilization_reports_the_configured_capacity_and_rates_remain_finite,
        ),
        (
            "supported expiration construction",
            expiration_is_checked_before_cache_construction,
        ),
    ]);
}
