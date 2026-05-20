//! `DisplayList` -- recorded sequence of drawing commands.
//!
//! This module provides the [`DisplayList`] type which records drawing
//! commands from a [`crate::Canvas`] for later execution by the GPU
//! backend. This follows the Command Pattern -- record now, execute
//! later.
//!
//! # Architecture
//!
//! ```text
//! Canvas::draw_rect() → DisplayList::push(DrawRect) → PictureLayer → WgpuPainter
//! ```
//!
//! # Concern split (Mythos chain U5)
//!
//! The 2,434-LOC `display_list.rs` god module was split into six
//! concern-based submodules:
//!
//! - [`command`]      -- 29-variant `DrawCommand` enum + `CommandKind`.
//! - [`command_ops`]  -- `DrawCommand` impl block (with_opacity, bounds, transform, paint, kind, is_*, apply_transform).
//! - [`sealed`]       -- sealed extension-trait pair (`DisplayListCore` + `DisplayListExt`) + 4 blanket impls.
//! - [`stats`]        -- `DisplayListStats` struct + Display impl.
//! - [`hit_region`]   -- `PointerEvent` + `PointerEventKind` + `HitRegion` + `HitRegionHandler`.
//!
//! This module (`mod.rs`) carries the `DisplayList` struct itself,
//! mutation methods (`apply_transform`/`filter`/`map`/`to_opacity`/
//! `clear`), iteration adapters (`iter`/`iter_mut`), `IntoIterator` +
//! `Index`/`IndexMut` + `AsRef`/`AsMut` impls, and the
//! `pub(crate) push` + `pub(crate) append` entry points used by
//! `Canvas`.

use std::ops::{Index, IndexMut};

use flui_types::geometry::{Matrix4, Pixels, Rect};

pub mod command;
pub mod command_ops;
pub mod hit_region;
pub mod sealed;
pub mod stats;

// Re-export the public surface.
pub use command::{CommandKind, DrawCommand};
pub use hit_region::{HitRegion, HitRegionHandler, PointerEvent, PointerEventKind};
pub use sealed::{DisplayListCore, DisplayListExt};
pub use stats::DisplayListStats;

// Re-exports from flui_types::painting that are part of the
// `display_list` public API surface.
pub use flui_types::painting::{
    BlendMode, Clip, ClipOp, FilterQuality, Paint, PointMode, Shader, TextureId,
    effects::ImageFilter,
    image::{ColorFilter, ImageRepeat},
};

/// A recorded sequence of drawing commands.
///
/// `DisplayList` is immutable after recording from the public API and
/// can be replayed multiple times by the engine. It is the output of
/// `Canvas` and the input to `PictureLayer`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayList {
    /// Drawing commands in order.
    pub(crate) commands: Vec<DrawCommand>,

    /// Cached bounds of all drawing.
    pub(crate) bounds: Rect<Pixels>,

    /// Hit-testable regions with event handlers.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) hit_regions: Vec<HitRegion>,
}

impl DisplayList {
    /// Creates a new empty display list.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: Rect::ZERO,
            hit_regions: Vec::new(),
        }
    }

    /// Returns an iterator over command references.
    pub fn iter(&self) -> std::slice::Iter<'_, DrawCommand> {
        self.commands.iter()
    }

    /// Returns an iterator over mutable command references.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, DrawCommand> {
        self.commands.iter_mut()
    }

    /// Add a hit-testable region with an event handler.
    ///
    /// Regions are tested in reverse order (last added = topmost).
    pub fn add_hit_region(&mut self, region: HitRegion) {
        self.hit_regions.push(region);
    }

    /// Get all hit regions.
    pub fn hit_regions(&self) -> &[HitRegion] {
        &self.hit_regions
    }

    /// Adds a command to the display list (internal).
    pub(crate) fn push(&mut self, command: DrawCommand) {
        if let Some(cmd_bounds) = command.bounds() {
            if self.commands.is_empty() {
                self.bounds = cmd_bounds;
            } else {
                self.bounds = self.bounds.union(&cmd_bounds);
            }
        }
        self.commands.push(command);
    }

    /// Returns an iterator over mutable command references.
    ///
    /// Demoted to `pub(crate)` in Mythos chain U10. External callers
    /// should mutate via the existing public API
    /// (`apply_transform`/`filter`/`map`/`to_opacity`) instead of
    /// touching commands directly.
    pub(crate) fn commands_mut(&mut self) -> impl Iterator<Item = &mut DrawCommand> {
        self.commands.iter_mut()
    }

    /// Returns statistics about this display list (overrides
    /// extension trait).
    ///
    /// This implementation includes hit_regions count, which the
    /// default extension trait implementation cannot access.
    pub fn stats(&self) -> DisplayListStats {
        let total = self.commands.len();
        let mut draw = 0;
        let mut clip = 0;
        let mut effect = 0;
        let mut layer = 0;

        for cmd in &self.commands {
            match cmd.kind() {
                CommandKind::Draw => draw += 1,
                CommandKind::Clip => clip += 1,
                CommandKind::Effect => effect += 1,
                CommandKind::Layer => layer += 1,
            }
        }

        let shapes = self.commands.iter().filter(|c| c.is_shape()).count();
        let images = self.commands.iter().filter(|c| c.is_image()).count();
        let text = self.commands.iter().filter(|c| c.is_text()).count();

        DisplayListStats {
            total,
            draw,
            clip,
            effect,
            layer,
            shapes,
            images,
            text,
            hit_regions: self.hit_regions.len(),
        }
    }

    /// Applies a transform to all commands in this display list.
    ///
    /// Modifies commands in-place. Useful for positioning cached
    /// display lists.
    pub fn apply_transform(&mut self, transform: Matrix4) {
        for cmd in &mut self.commands {
            cmd.apply_transform(transform);
        }
        self.recalculate_bounds();
    }

    /// Recalculates the bounds from all commands.
    fn recalculate_bounds(&mut self) {
        self.bounds = Rect::ZERO;
        for cmd in &self.commands {
            if let Some(cmd_bounds) = cmd.bounds() {
                if self.bounds == Rect::ZERO {
                    self.bounds = cmd_bounds;
                } else {
                    self.bounds = self.bounds.union(&cmd_bounds);
                }
            }
        }
    }

    /// Filters commands, keeping only those that satisfy the
    /// predicate.
    #[must_use = "filter returns a new DisplayList and does not modify the original"]
    pub fn filter<F>(&self, predicate: F) -> Self
    where
        F: Fn(&DrawCommand) -> bool,
    {
        let commands: Vec<_> = self
            .commands
            .iter()
            .filter(|cmd| predicate(cmd))
            .cloned()
            .collect();

        let mut result = Self {
            commands,
            bounds: Rect::ZERO,
            hit_regions: self.hit_regions.clone(),
        };
        result.recalculate_bounds();
        result
    }

    /// Maps each command through a function.
    #[must_use = "map returns a new DisplayList and does not modify the original"]
    pub fn map<F>(&self, f: F) -> Self
    where
        F: Fn(&DrawCommand) -> DrawCommand,
    {
        let commands: Vec<_> = self.commands.iter().map(f).collect();

        let mut result = Self {
            commands,
            bounds: Rect::ZERO,
            hit_regions: self.hit_regions.clone(),
        };
        result.recalculate_bounds();
        result
    }

    /// Clears all commands and hit regions (for pooling/reuse).
    pub fn clear(&mut self) {
        self.commands.clear();
        self.bounds = Rect::ZERO;
        self.hit_regions.clear();
    }

    /// Appends all commands from another DisplayList (zero-copy
    /// move).
    ///
    /// # Performance
    ///
    /// - O(1) if self is empty (just swap the vectors).
    /// - O(N) otherwise where N = `other.len()` (but no cloning,
    ///   just move).
    #[tracing::instrument(skip(self, other), fields(
        parent_len = self.commands.len(),
        child_len = other.commands.len(),
    ))]
    pub(crate) fn append(&mut self, mut other: DisplayList) {
        if self.commands.is_empty() {
            tracing::trace!("Using fast path: vector swap (O(1))");
            std::mem::swap(&mut self.commands, &mut other.commands);
            self.bounds = other.bounds;
        } else if !other.commands.is_empty() {
            tracing::trace!(
                commands_to_append = other.commands.len(),
                "Using append path (O(N))"
            );
            self.commands.append(&mut other.commands);

            if !other.bounds.is_empty() {
                self.bounds = self.bounds.union(&other.bounds);
            }
        }

        if !other.hit_regions.is_empty() {
            self.hit_regions.append(&mut other.hit_regions);
        }

        tracing::debug!(
            total_commands = self.commands.len(),
            "DisplayList append complete"
        );
    }

    /// Apply opacity to all commands, creating a new DisplayList.
    ///
    /// # Performance
    ///
    /// O(N) where N is the number of commands.
    #[must_use = "to_opacity returns a new DisplayList and does not modify the original"]
    #[tracing::instrument(skip(self), fields(
        commands = self.commands.len(),
        opacity = opacity,
    ))]
    pub fn to_opacity(&self, opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);

        tracing::debug!(
            commands = self.commands.len(),
            clamped_opacity = opacity,
            "Applying opacity to DisplayList"
        );

        let commands = self
            .commands
            .iter()
            .map(|cmd| cmd.with_opacity(opacity))
            .collect();

        Self {
            commands,
            bounds: self.bounds,
            hit_regions: self.hit_regions.clone(),
        }
    }
}

impl Default for DisplayList {
    fn default() -> Self {
        Self::new()
    }
}

// ===== AsRef / AsMut Implementations =====

/// Allow zero-cost conversion from DisplayList to slice of commands.
impl AsRef<[DrawCommand]> for DisplayList {
    fn as_ref(&self) -> &[DrawCommand] {
        &self.commands
    }
}

/// Allow zero-cost mutable conversion from DisplayList to slice of
/// commands.
impl AsMut<[DrawCommand]> for DisplayList {
    fn as_mut(&mut self) -> &mut [DrawCommand] {
        &mut self.commands
    }
}

// ===== IntoIterator Implementation =====

impl<'a> IntoIterator for &'a DisplayList {
    type Item = &'a DrawCommand;
    type IntoIter = std::slice::Iter<'a, DrawCommand>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.iter()
    }
}

impl<'a> IntoIterator for &'a mut DisplayList {
    type Item = &'a mut DrawCommand;
    type IntoIter = std::slice::IterMut<'a, DrawCommand>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.iter_mut()
    }
}

// ===== Index Trait Implementation =====

impl Index<usize> for DisplayList {
    type Output = DrawCommand;

    fn index(&self, index: usize) -> &Self::Output {
        &self.commands[index]
    }
}

impl IndexMut<usize> for DisplayList {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.commands[index]
    }
}
