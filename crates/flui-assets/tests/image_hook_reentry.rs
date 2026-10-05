//! Real decoder-hook reentry runs alone because image registrations are global.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_assets::{AssetHandle, AssetKey, AssetRegistryBuilder, Image, ImageAsset};

#[test]
fn registered_image_hook_can_await_same_key_spawned_work() {
    let registry = Arc::new(AssetRegistryBuilder::new().with_default_capacity().build());
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    let hook_registry = Arc::clone(&registry);
    let (observed_tx, observed_rx) = std::sync::mpsc::channel::<AssetHandle<Image, AssetKey>>();
    let bytes = include_bytes!("fixtures/tiny.png");
    let extension = std::ffi::OsString::from("flui-reentrant-png");
    assert!(image::hooks::register_decoding_hook(
        extension.clone(),
        Box::new(move |reader| {
            if hook_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                let child_registry = Arc::clone(&hook_registry);
                let (child_tx, child_rx) = std::sync::mpsc::channel();
                tokio::spawn(async move {
                    let result = child_registry
                        .load(ImageAsset::from_bytes("hooked-image", bytes.to_vec()))
                        .await;
                    if let Ok(child) = &result {
                        let cached = child_registry.get::<ImageAsset>(child.key()).await;
                        assert!(
                            cached.is_some_and(|cached| cached.ptr_eq(child)),
                            "successful child publishes before the parent completes"
                        );
                    }
                    let _ = child_tx.send(result);
                });
                // A synchronous decoder callback cannot await a future directly.
                // Bound its bridge so a restored singleflight cycle fails, not hangs.
                let child = child_rx
                    .recv_timeout(Duration::from_secs(3))
                    .map_err(|error| {
                        image::ImageError::IoError(std::io::Error::other(format!(
                            "same-key image hook child deadline: {error}"
                        )))
                    })?
                    .map_err(|error| image::ImageError::IoError(std::io::Error::other(error)))?;
                observed_tx
                    .send(child)
                    .expect("child handle observer remains alive");
            }
            image::codecs::png::PngDecoder::new(reader)
                .map(|decoder| Box::new(decoder) as Box<dyn image::ImageDecoder + '_>)
        })
    ));
    image::hooks::register_format_detection_hook(extension, &bytes[..8], None);

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("two-thread image runtime starts")
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let outer = registry
                    .load(ImageAsset::from_bytes("hooked-image", bytes.to_vec()))
                    .await
                    .expect("registered hook and its same-key child must finish");
                let child = observed_rx
                    .try_recv()
                    .expect("hook retains the child handle");
                assert_eq!(
                    calls.load(Ordering::Relaxed),
                    2,
                    "both independent decodes retain registered-hook behavior"
                );
                assert_eq!((outer.width(), outer.height()), (4, 2));
                assert_eq!((child.width(), child.height()), (4, 2));
                assert!(
                    !outer.ptr_eq(&child),
                    "parent and child own independent data"
                );
                let cached = registry
                    .load(ImageAsset::from_bytes("hooked-image", bytes.to_vec()))
                    .await
                    .expect("late parent completion is cached");
                assert!(cached.ptr_eq(&outer));
                assert_eq!(calls.load(Ordering::Relaxed), 2, "cache hit skips decoding");
            })
            .await
            .expect("image-hook reentry must finish within five seconds");
        });
}
