use super::*;

/// A non-primary presentation's pipeline must be seeded with ITS
/// OWN window's `scale_factor()` before construction — the same
/// DPR-before-first-frame ordering `UiRealm::construct` already
/// gives this realm's primary presentation (there, the caller reads
/// the window's scale factor and passes it in explicitly; here,
/// `assemble_presentation` already holds the window, so it reads
/// the value directly instead).
///
/// If reverted (the `set_device_pixel_ratio` seeding call removed
/// from `assemble_presentation`): this fails — the readback
/// observes the `PipelineOwner::new()` default (`1.0`) instead of
/// the window's real `2.5`.
#[test]
fn non_primary_presentation_pipeline_is_seeded_with_its_own_windows_scale_factor() {
    let mut realm = UiRealm::for_test();
    let window_b: Arc<dyn PlatformWindow> = Arc::new(
        crate::testing::TestWindow::new()
            .with_id(42)
            .with_scale_factor(2.5),
    );
    let presentation_b = realm.assemble_presentation(window_b);
    let b_id = realm.install_presentation(presentation_b);

    let dpr = realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .pipeline()
        .with(flui_rendering::pipeline::PipelineOwner::device_pixel_ratio);

    assert_eq!(
        dpr, 2.5,
        "B's pipeline must be seeded with its own window's scale factor before \
         construction, not the PipelineOwner default"
    );
}
