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
