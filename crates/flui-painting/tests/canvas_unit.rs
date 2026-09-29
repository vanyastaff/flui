//! Canvas unit tests extracted from `crates/flui-painting/src/canvas/mod.rs`
//! when the module was split into concern-based submodules.
//!
//! These tests live as integration tests so the new submodule split
//! (`canvas/{mod,state,transform,clipping,drawing,scoped,composition,sugar}.rs`)
//! does not carry inline `#[cfg(test)] mod tests` blocks for surface
//! that is already exercised through the public API.

use flui_painting::Canvas;

#[test]
fn test_canvas_save_restore() {
    let mut canvas = Canvas::new();

    assert_eq!(canvas.save_count(), 1);

    canvas.save();
    assert_eq!(canvas.save_count(), 2);

    canvas.translate(50.0, 50.0);

    canvas.save();
    assert_eq!(canvas.save_count(), 3);

    canvas.restore();
    assert_eq!(canvas.save_count(), 2);

    canvas.restore();
    assert_eq!(canvas.save_count(), 1);
}

/// `Canvas::finish` wires a `debug_assert!` to
/// catch unrestored `save()` calls during test runs. Release builds
/// preserve Flutter parity (silent finalisation via `tracing::warn!`).
///
/// This test fires only when `debug_assertions` is on (cargo test
/// default). Release-build `cargo test --release` would skip the
/// panic-expectation, matching the documented per-mode behavior.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "unrestored save() calls")]
fn test_canvas_finish_panics_in_debug_on_unrestored_save() {
    let mut canvas = Canvas::new();
    canvas.save();
    canvas.translate(50.0, 50.0);
    // No matching restore() -- save_stack has 1 entry at finish() time.
    let _ = canvas.finish();
}

// ===== draw_polyline =====
