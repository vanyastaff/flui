//! An asset registry gives back its completed entries under memory pressure.

use flui_assets::{Asset, AssetRegistryBuilder, FontAsset};
use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};

fn font(name: &str) -> FontAsset {
    FontAsset::from_bytes(
        name,
        include_bytes!("../../flui-painting/assets/fonts/probe-sans-400.ttf").to_vec(),
    )
}

#[tokio::test]
async fn trimming_drops_completed_entries_but_not_handles_already_given_out() {
    for level in [MemoryPressureLevel::Moderate, MemoryPressureLevel::Critical] {
        let registry = AssetRegistryBuilder::new().with_default_capacity().build();
        let key = font("trimmed").key();
        let before = registry.load(font("trimmed")).await.expect("loads");
        assert!(registry.get::<FontAsset>(&key).await.is_some());

        registry.reclaim(level);

        assert!(
            registry.get::<FontAsset>(&key).await.is_none(),
            "{level:?} dropped the cached entry"
        );
        assert!(
            before.downgrade().upgrade().is_some(),
            "{level:?} left the handle's data alone"
        );
        let after = registry.load(font("trimmed")).await.expect("reloads");
        assert!(!before.ptr_eq(&after), "{level:?} reloaded from the source");
    }
}
