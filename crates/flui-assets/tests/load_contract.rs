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
    ]);
}
