//! Backend-agnostic frame-driver trait.
//!
//! [`RasterBackend`] is the swap point for the rendering backend. A future
//! Vello or software backend implements this trait; lyon and wgpu are
//! internal implementation details of the current wgpu backend.
//!
//! # Design notes
//!
//! - Constructors are deliberately excluded: backend construction is
//!   window-specific and async, so it stays on the concrete type.
//! - The trait is dyn-compatible (no generics, no `async` in methods).
//! - Feature gating: the trait itself is unconditional; only the
//!   `impl RasterBackend for Renderer` is gated on `wgpu-backend`.

use flui_layer::Scene;
use flui_types::geometry::{Pixels, Rect};

use crate::error::EngineError;

/// The hook a [`RasterBackend`] runs immediately before every present —
/// see [`RasterBackend::set_pre_present_hook`].
pub type PrePresentHook = Box<dyn FnMut() + Send>;

/// Frame-driver interface for a rendering backend.
///
/// Covers the per-frame and surface-management methods the application layer
/// calls on a renderer. Constructors are excluded — backend creation is
/// window-specific and async, so it lives on the concrete type.
///
/// This is the swap point: a future Vello or software backend implements
/// this trait while the application layer changes only the construction line.
/// Lyon and wgpu are internal details of the wgpu implementation.
///
/// The trait is dyn-compatible (no generic parameters, no `async` methods).
/// `Send` is a supertrait (ADR-0045 decision 1): the raster owner moves the
/// backend onto its own thread once at lane construction, so every backend
/// implementation must cross that boundary. Note that this does **not** make
/// `dyn RasterBackend` itself `Send`: Rust never infers an auto-trait bound
/// onto a trait object from a supertrait, so a use site that needs the
/// object to cross a thread must spell out `dyn RasterBackend + Send`. A use
/// site that does not — a `Box<dyn RasterBackend>` staying on one thread —
/// needs nothing extra.
pub trait RasterBackend: Send {
    /// Render a [`Scene`] to the surface.
    ///
    /// Traverses the scene's `LayerTree` and dispatches each layer's
    /// display-list commands through the GPU backend. Returns whether the
    /// frame actually reached `present()` — `false` covers every skip path
    /// (no damage, occluded surface) that returns successfully without
    /// presenting, and therefore without a vsync block.
    fn render_scene(&mut self, scene: &Scene) -> Result<bool, EngineError>;

    /// Resize the surface to the given physical pixel dimensions.
    fn resize(&mut self, width: u32, height: u32);

    /// Returns `true` if the GPU device has been lost.
    ///
    /// After a TDR, driver crash, or GPU hardware failure the device-lost
    /// flag is set. The caller should attempt recovery via the concrete
    /// type's `recover()` method (excluded from this trait — it is async
    /// and takes a window handle, which are backend-specific concerns).
    fn is_device_lost(&self) -> bool;

    /// Mark a screen region as dirty (needs repaint on the next frame).
    fn mark_dirty(&mut self, rect: Rect<Pixels>);

    /// Mark the entire screen as needing repaint.
    fn mark_full_repaint(&mut self);

    /// Returns `true` if the renderer has pending damage to paint.
    fn has_damage(&self) -> bool;

    /// Current surface size as `(width, height)` in physical pixels.
    ///
    /// Returns `(0, 0)` when no surface is configured (e.g. offscreen).
    fn size(&self) -> (u32, u32);

    /// Reconfigure the surface after an outdated or lost surface error.
    ///
    /// Called automatically by `render_scene` on `Outdated`/`Lost`, but
    /// may also be called manually when the surface needs reconfiguration
    /// (e.g. format change).
    fn reconfigure_surface(&mut self) -> Result<(), EngineError>;

    /// Install the hook this backend runs immediately before every present
    /// (`None` uninstalls). The platform frame-pacing seam: an embedder
    /// hands in its window's `pre_present_notify`, so the windowing system
    /// learns a present is about to happen — on Wayland that arms the
    /// surface's frame callback, which is what makes the next redraw
    /// request compositor-paced and an occluded surface silent. It runs
    /// only when a present WILL follow, never on a skip path: a frame
    /// callback requested with no commit behind it would leave winit
    /// withholding every later `RedrawRequested` (its Wayland event loop
    /// waits for the callback that commit would have produced).
    ///
    /// Runs on whichever thread presents — the owner thread on the inline
    /// lane. A threaded lane would run it on the raster thread; ADR-0045
    /// decision 5's rule against raster-thread platform calls is about
    /// AppKit main-thread affinity, and this hook is inert there.
    ///
    /// Default: ignored. A backend that never presents (offscreen,
    /// scripted doubles) has nothing to notify.
    fn set_pre_present_hook(&mut self, hook: Option<PrePresentHook>) {
        drop(hook);
    }
}

// ---------------------------------------------------------------------------
// wgpu backend implementation
// ---------------------------------------------------------------------------

#[cfg(feature = "wgpu-backend")]
impl RasterBackend for crate::wgpu::Renderer {
    fn render_scene(&mut self, scene: &Scene) -> Result<bool, EngineError> {
        self.render_scene(scene)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.resize(width, height);
    }

    fn is_device_lost(&self) -> bool {
        self.is_device_lost()
    }

    fn mark_dirty(&mut self, rect: Rect<Pixels>) {
        self.mark_dirty(rect);
    }

    fn set_pre_present_hook(&mut self, hook: Option<PrePresentHook>) {
        self.set_pre_present_hook(hook);
    }

    fn mark_full_repaint(&mut self) {
        self.mark_full_repaint();
    }

    fn has_damage(&self) -> bool {
        self.has_damage()
    }

    fn size(&self) -> (u32, u32) {
        self.size()
    }

    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        self.reconfigure_surface()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal backend used only to exercise `RasterBackend` as a trait
    /// object — not a behavioral fake for the wgpu backend. Every method
    /// but `mark_full_repaint`/`has_damage` is a no-op / fixed return.
    #[derive(Default)]
    struct NoOpBackend {
        damage: bool,
    }

    impl RasterBackend for NoOpBackend {
        fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
            Ok(false)
        }

        fn resize(&mut self, _width: u32, _height: u32) {}

        fn is_device_lost(&self) -> bool {
            false
        }

        fn mark_dirty(&mut self, _rect: Rect<Pixels>) {
            self.damage = true;
        }

        fn mark_full_repaint(&mut self) {
            self.damage = true;
        }

        fn has_damage(&self) -> bool {
            self.damage
        }

        fn size(&self) -> (u32, u32) {
            (0, 0)
        }

        fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
            Ok(())
        }
    }

    /// Moves a `Box<dyn RasterBackend + Send>` to a real OS thread and
    /// calls through the trait object there.
    ///
    /// ADR-0045 decision 1 requires `RasterBackend: Send` and
    /// `dyn RasterBackend + Send` to stay nameable and dyn-safe — the exact
    /// shape the raster lane needs once it threads a boxed backend across
    /// the owner boundary. Naming `+ Send` on the trait-object type is not
    /// implied by the `Send` supertrait alone (Rust does not infer a
    /// supertrait auto-trait bound onto a bare `dyn Trait`), so this pins
    /// both properties together rather than either one in isolation. A
    /// regression in either — the trait losing dyn-safety, or a backend
    /// silently losing `Send` — fails this test, not just a hypothetical.
    #[test]
    fn raster_backend_is_dyn_safe_and_moves_across_threads() {
        let backend: Box<dyn RasterBackend + Send> = Box::new(NoOpBackend::default()); // PORT-CHECK-OK-DYN: proves ADR-0045 decision 1's object-safety requirement (dyn RasterBackend + Send stays nameable/dyn-safe); test-only, no production dyn RasterBackend site exists today.
        let handle = std::thread::spawn(move || {
            let mut backend = backend;
            assert!(!backend.has_damage(), "fresh backend starts with no damage");
            backend.mark_full_repaint();
            assert!(
                backend.has_damage(),
                "mark_full_repaint must be observable through the trait object \
                 after crossing the thread boundary"
            );
            backend.size()
        });
        assert_eq!(handle.join().expect("owner thread must not panic"), (0, 0));
    }
}
