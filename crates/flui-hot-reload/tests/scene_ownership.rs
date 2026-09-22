//! Exercise the actual generated allocation boundary without loading a DSO.
#![expect(
    unsafe_code,
    reason = "tests the documented raw scene ownership protocol"
)]

use flui_hot_reload::Scene;
use flui_layer::{AnnotatedRegionLayer, Layer, LayerTree};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Payload;
impl Drop for Payload {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

fn build_scene(_: f32, _: f32) -> Scene {
    Scene::new(LayerTree::new(Layer::AnnotatedRegion(
        AnnotatedRegionLayer::sized_by_parent(Arc::new(Payload)),
    )))
}

flui_hot_reload::scene_plugin!(build_scene);

#[test]
fn both_ownership_paths_destroy_the_payload_once_and_null_is_a_noop() {
    let original = flui_scene_build(10.0, 20.0);
    assert!(!original.is_null());
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);
    // SAFETY: unique initialized allocation from this image, never moved or consumed.
    unsafe { flui_scene_drop(original) };
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);

    let allocation = flui_scene_build(30.0, 40.0);
    assert!(!allocation.is_null());
    // SAFETY: this aligned allocation contains one initialized Scene; read exactly once.
    let moved = unsafe { allocation.cast::<Scene>().read() };
    // SAFETY: the scene was moved out once and this live allocation is uniquely owned.
    unsafe { flui_scene_free(allocation) };
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(moved.tree().len(), 1);
    drop(moved);
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);

    // SAFETY: both entry points explicitly accept null without any allocation ownership.
    unsafe {
        flui_scene_drop(std::ptr::null_mut());
        flui_scene_free(std::ptr::null_mut());
    }
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
}
