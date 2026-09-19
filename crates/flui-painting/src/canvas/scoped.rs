//! `with_*` helpers that pair a `save` (or `save_layer`) with its `restore`
//! around a closure, so the two cannot drift apart at a call site. A panic
//! inside the closure unwinds past the `restore`; the canvas is not reused
//! after a panic, so nothing depends on it.

use flui_types::{
    geometry::{Matrix4, Pixels, RRect, Rect},
    painting::{BlendMode, Path},
};

use super::Canvas;

impl Canvas {
    fn with_save<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.save();
        let result = f(self);
        self.restore();
        result
    }

    /// Executes a closure with an arbitrary transform applied.
    #[inline]
    pub fn with_transform<T, F, R>(&mut self, transform: T, f: F) -> R
    where
        T: Into<Matrix4>,
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.transform(transform);
            f(c)
        })
    }

    /// Executes a closure with a clipping rectangle applied.
    #[inline]
    pub fn with_clip_rect<F, R>(&mut self, rect: Rect<Pixels>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.clip_rect(rect);
            f(c)
        })
    }

    /// Executes a closure with a clipping rounded rectangle applied.
    #[inline]
    pub fn with_clip_rrect<F, R>(&mut self, rrect: RRect, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.clip_rrect(rrect);
            f(c)
        })
    }

    /// Executes a closure with a clipping path applied.
    #[inline]
    pub fn with_clip_path<F, R>(&mut self, path: &Path, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.clip_path(path);
            f(c)
        })
    }

    /// Executes a closure with a compositing layer for opacity effects.
    ///
    /// Creates an offscreen buffer; use sparingly (GPU overhead).
    #[inline]
    pub fn with_opacity<F, R>(&mut self, opacity: f32, bounds: Option<Rect<Pixels>>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.save_layer_opacity(bounds, opacity);
        let result = f(self);
        self.restore();
        result
    }

    /// Executes a closure with a compositing layer for blend mode
    /// effects.
    #[inline]
    pub fn with_blend_mode<F, R>(
        &mut self,
        blend_mode: BlendMode,
        bounds: Option<Rect<Pixels>>,
        f: F,
    ) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.save_layer_blend(bounds, blend_mode);
        let result = f(self);
        self.restore();
        result
    }
}
