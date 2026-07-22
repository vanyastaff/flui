//! Canvas scoped operations: 12 `with_*` helpers that wrap
//! `save()`/`restore()` around a closure.
//!
//! These were extracted from the 3,305-LOC `canvas.rs` god
//! module. Each scoped helper compiles to a direct `save() + body +
//! restore()` sequence -- zero overhead vs. manual save/restore.
//!
//! Scoped helpers are safer than manual save/restore because the
//! canvas state is automatically restored after the closure (even if
//! the closure panics, the `restore()` is in a `Drop`-equivalent
//! position via the call-stack unwinding).

use flui_types::{
    geometry::{Matrix4, Pixels, RRect, Rect},
    painting::{BlendMode, Path},
};

use super::Canvas;

impl Canvas {
    /// Executes a closure with automatic save/restore.
    #[inline]
    pub fn with_save<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.save();
        let result = f(self);
        self.restore();
        result
    }

    /// Executes a closure with a translated coordinate system.
    #[inline]
    pub fn with_translate<F, R>(&mut self, dx: f32, dy: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.translate(dx, dy);
            f(c)
        })
    }

    /// Executes a closure with a rotated coordinate system.
    #[inline]
    pub fn with_rotate<F, R>(&mut self, radians: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.rotate(radians);
            f(c)
        })
    }

    /// Executes a closure with a rotated coordinate system around a
    /// pivot point.
    #[inline]
    pub fn with_rotate_around<F, R>(&mut self, radians: f32, pivot_x: f32, pivot_y: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.rotate_around(radians, pivot_x, pivot_y);
            f(c)
        })
    }

    /// Executes a closure with a scaled coordinate system.
    #[inline]
    pub fn with_scale<F, R>(&mut self, factor: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.scale_uniform(factor);
            f(c)
        })
    }

    /// Executes a closure with a non-uniform scaled coordinate system.
    #[inline]
    pub fn with_scale_xy<F, R>(&mut self, sx: f32, sy: f32, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_save(|c| {
            c.scale_xy(sx, sy);
            f(c)
        })
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
