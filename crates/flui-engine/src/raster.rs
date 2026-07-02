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
pub trait RasterBackend {
    /// Render a [`Scene`] to the surface.
    ///
    /// Traverses the scene's `LayerTree` and dispatches each layer's
    /// display-list commands through the GPU backend.
    fn render_scene(&mut self, scene: &Scene) -> Result<(), EngineError>;

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
}

// ---------------------------------------------------------------------------
// wgpu backend implementation
// ---------------------------------------------------------------------------

#[cfg(feature = "wgpu-backend")]
impl RasterBackend for crate::wgpu::Renderer {
    fn render_scene(&mut self, scene: &Scene) -> Result<(), EngineError> {
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
