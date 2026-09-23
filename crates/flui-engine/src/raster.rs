//! The frame-driver trait the application layer calls on a renderer.
//!
//! [`RasterBackend`] has one production implementor, [`Renderer`], and
//! exists so `flui-app`'s frame loop, raster lane, and device-recovery
//! paths can be driven by a scripted fake with no GPU (every other
//! implementor is a test double). It is a test seam, not a plugin point:
//! wgpu is the engine and no second backend is planned.
//!
//! [`Renderer`]: crate::Renderer
//! # Design notes
//!
//! - Constructors are deliberately excluded: backend construction is
//!   window-specific and async, so it stays on the concrete type.
//! - The trait is dyn-compatible (no generics, no `async` in methods).

use flui_layer::Scene;
use flui_types::geometry::{Pixels, Rect};

use crate::error::EngineError;

/// The hook a [`RasterBackend`] runs immediately before every present —
/// see [`RasterBackend::set_pre_present_hook`].
pub type PrePresentHook = Box<dyn FnMut() + Send>;

/// What a [`RasterBackend::render_scene`] call did with the frame it was
/// handed.
///
/// A three-state answer rather than the `bool` this used to be, because the
/// two ways a frame fails to reach `present()` are not the same event and
/// have opposite consequences for the caller's pacing:
///
/// - [`NoDamage`](Self::NoDamage) — nothing was owed. The caller's work is
///   genuinely done and the loop may park.
/// - [`NotShown`](Self::NotShown) — content *was* owed and could not be put
///   on screen. The work was consumed producing the scene and the screen
///   never saw it, so a caller that treats this as "done" loses the frame
///   permanently: on a stack whose redraw rate is bounded by its own
///   fallback deadline (ADR-0058), a frame classified this way is the last
///   wake the loop gets, and a cold start that hits it is a window that
///   stays blank with nothing left to come back for.
///
/// A backend that cannot distinguish the two reports
/// [`NoDamage`](Self::NoDamage) for every skipped frame — the conservative
/// answer, since it never invents a retry the backend did not ask for.
///
/// Deliberately **not** `#[non_exhaustive`], unlike the sibling protocol
/// types around it. Every consumer of this value has to decide what an
/// answer means for pacing, and the bug this type was introduced to fix was
/// exactly a *silent* conflation of two of them: a wildcard arm that folded
/// an unknown future state into "nothing owed" would institutionalise the
/// same mistake for the next state. Exhaustive means a fourth answer cannot
/// be added without the compiler naming every site that must classify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentDisposition {
    /// The frame reached `present()`.
    Presented,
    /// Nothing was owed: the backend had no damage to paint.
    NoDamage,
    /// Content was owed and could not be shown — the surface reported
    /// itself unavailable (occluded), its owner had released it for the
    /// duration of a suspend, or the frame unwound before it could
    /// present.
    NotShown,
}

impl PresentDisposition {
    /// Whether the frame reached the screen.
    #[must_use]
    pub fn was_shown(self) -> bool {
        matches!(self, Self::Presented)
    }

    /// Whether the frame owed content it never managed to show, and so
    /// needs a retry rather than being counted as finished.
    #[must_use]
    pub fn is_withheld(self) -> bool {
        matches!(self, Self::NotShown)
    }
}

/// Frame-driver interface for a rendering backend.
///
/// Covers the per-frame and surface-management methods the application layer
/// calls on a renderer. Constructors are excluded — backend creation is
/// window-specific and async, so it lives on the concrete type.
///
/// [`Renderer`](crate::Renderer) is the one production implementor; the
/// others are test doubles that let the application layer's frame loop run
/// without a GPU.
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
    /// display-list commands through the GPU backend. Returns what became of
    /// the frame — see [`PresentDisposition`] for why the answer carries
    /// more than "did it present": only [`PresentDisposition::Presented`]
    /// means a vsync block happened, and only
    /// [`PresentDisposition::NotShown`] means the caller's work was consumed
    /// without reaching the screen and owes a retry. See the concrete
    /// backend's own doc for which of the skip paths it can tell apart.
    fn render_scene(&mut self, scene: &Scene) -> Result<PresentDisposition, EngineError>;

    /// Resize the surface to the given physical pixel dimensions.
    fn resize(&mut self, width: u32, height: u32);

    /// Returns `true` if the GPU device has been lost.
    ///
    /// After a TDR, driver crash, or GPU hardware failure the device-lost
    /// flag is set. The caller should attempt recovery via the concrete
    /// type's `recover()` method (excluded from this trait — it is async
    /// and takes a window handle, which are backend-specific concerns).
    #[must_use]
    fn is_device_lost(&self) -> bool;

    /// Mark a screen region as dirty (needs repaint on the next frame).
    fn mark_dirty(&mut self, rect: Rect<Pixels>);

    /// Mark the entire screen as needing repaint.
    fn mark_full_repaint(&mut self);

    /// Returns `true` if the renderer has pending damage to paint.
    #[must_use]
    fn has_damage(&self) -> bool;

    /// Current surface size as `(width, height)` in physical pixels.
    ///
    /// Returns `(0, 0)` when no surface is configured (e.g. offscreen).
    #[must_use]
    fn size(&self) -> (u32, u32);

    /// Reconfigure the surface after an outdated or lost surface error.
    ///
    /// Called automatically by `render_scene` on `Outdated`/`Lost`, but
    /// may also be called manually when the surface needs reconfiguration
    /// (e.g. format change).
    ///
    /// While a windowed backend holds no surface because it released one, the
    /// call is a no-op rather than an error: there is nothing to reconfigure
    /// and the release is deliberate. The wgpu `Renderer` never fails here;
    /// the `Result` is for a backend whose reconfigure can.
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
impl RasterBackend for crate::Renderer {
    fn render_scene(&mut self, scene: &Scene) -> Result<PresentDisposition, EngineError> {
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
        self.reconfigure_surface();
        Ok(())
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
        fn render_scene(&mut self, _scene: &Scene) -> Result<PresentDisposition, EngineError> {
            Ok(PresentDisposition::NoDamage)
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
        let backend: Box<dyn RasterBackend + Send> = Box::new(NoOpBackend::default()); // proves ADR-0045 decision 1's object-safety requirement (dyn RasterBackend + Send stays nameable/dyn-safe); test-only, no production dyn RasterBackend site exists today.
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
