//! Validation gates registry admission, including cache hits.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_assets::{Asset, AssetError, AssetRegistryBuilder};

struct CheckedAsset {
    accepted: bool,
    loads: Arc<AtomicUsize>,
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
