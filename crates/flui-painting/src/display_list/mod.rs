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
//! concern-based files: this `mod.rs` plus five submodules.
//!
//! - `mod.rs` (this file) -- the `DisplayList` struct, mutation
//!   methods (`apply_transform`/`filter`/`map`/`to_opacity`/`clear`),
//!   iteration adapters, and the `pub(crate)` `push`/`append` entry
//!   points used by `Canvas`.
//! - [`command`]      -- 29-variant `DrawCommand` enum + `CommandKind`.
//! - [`command_ops`]  -- `DrawCommand` impl block (with_opacity, bounds, transform, paint, kind, is_*, apply_transform).
//! - [`sealed`]       -- sealed extension-trait pair (`DisplayListCore` + `DisplayListExt`) + 4 blanket impls.
//! - [`stats`]        -- `DisplayListStats` struct + Display impl.
//!
//! This module (`mod.rs`) carries the `DisplayList` struct itself,
//! mutation methods (`apply_transform`/`filter`/`map`/`to_opacity`/
//! `clear`), iteration adapters (`iter`/`iter_mut`), `IntoIterator` +
//! `Index`/`IndexMut` + `AsRef`/`AsMut` impls, and the
//! `pub(crate) push` + `pub(crate) append` entry points used by
//! `Canvas`.

use std::ops::{Index, IndexMut};

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_types::geometry::{Matrix4, Pixels, Rect};

pub mod command;
pub mod command_ops;
pub mod sealed;
pub mod stats;
pub mod summary;

// Re-export the public surface.
pub use command::{CommandKind, DrawCommand};
pub use sealed::{DisplayListCore, DisplayListExt};
pub use stats::DisplayListStats;

// Re-exports from flui_types::painting that are part of the
// `display_list` public API surface.
//
// REVIEW_BY: 2026-09-22 — audit P-12 cadence marker; mirrors the marker
// on `crates/flui-painting/src/lib.rs`. The canonical home of these
// types is `flui_types::painting`; this re-export is a convenience
// facade.
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
}

impl Diagnosticable for DisplayList {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("commands", self.commands.len());
        properties.add("bounds", format!("{:?}", self.bounds));
    }
}

impl DisplayList {
    /// Creates a new empty display list.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: Rect::ZERO,
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

    /// Returns statistics about this display list.
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
        }
    }

    /// Applies a transform to all commands in this display list.
    ///
    /// Modifies commands in-place. Useful for positioning cached
    /// display lists. Recursion into nested
    /// [`DrawCommand::ShaderMask`] / [`DrawCommand::BackdropFilter`]
    /// children is bounded by
    /// [`command_ops::MAX_EFFECT_DEPTH`].
    pub fn apply_transform(&mut self, transform: Matrix4) {
        self.apply_transform_depth(transform, 0);
    }

    /// Depth-counted recursion target for
    /// [`Self::apply_transform`]. Called by
    /// [`DrawCommand::apply_transform_depth`] when descending into a
    /// nested child `DisplayList`.
    pub(crate) fn apply_transform_depth(&mut self, transform: Matrix4, depth: usize) {
        for cmd in &mut self.commands {
            cmd.apply_transform_depth(transform, depth);
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
        };
        result.recalculate_bounds();
        result
    }

    /// Clears all commands (for pooling/reuse).
    pub fn clear(&mut self) {
        self.commands.clear();
        self.bounds = Rect::ZERO;
    }

    /// Appends all commands from another DisplayList (zero-copy
    /// move), unioning the cached bounds.
    ///
    /// Commands are self-contained (each carries its own transform),
    /// so concatenation preserves replay semantics. The
    /// fragment-composition paint walk uses this to merge adjacent
    /// inline paint runs into one picture instead of emitting a
    /// `PictureLayer` per render object.
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
    pub fn append(&mut self, mut other: DisplayList) {
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

        tracing::debug!(
            total_commands = self.commands.len(),
            "DisplayList append complete"
        );
    }

    /// Apply opacity to all commands, creating a new DisplayList.
    ///
    /// # Performance
    ///
    /// O(N) where N is the number of commands. Recursion into nested
    /// [`DrawCommand::ShaderMask`] / [`DrawCommand::BackdropFilter`]
    /// children is bounded by
    /// [`command_ops::MAX_EFFECT_DEPTH`].
    #[must_use = "to_opacity returns a new DisplayList and does not modify the original"]
    #[tracing::instrument(skip(self), fields(
        commands = self.commands.len(),
        opacity = opacity,
    ))]
    pub fn to_opacity(&self, opacity: f32) -> Self {
        self.to_opacity_depth(opacity, 0)
    }

    /// Depth-counted recursion target for [`Self::to_opacity`].
    ///
    /// Called by [`DrawCommand::with_opacity_depth`] when descending
    /// into a nested child `DisplayList`.
    pub(crate) fn to_opacity_depth(&self, opacity: f32, depth: usize) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);

        tracing::debug!(
            commands = self.commands.len(),
            clamped_opacity = opacity,
            depth = depth,
            "Applying opacity to DisplayList"
        );

        let commands = self
            .commands
            .iter()
            .map(|cmd| cmd.with_opacity_depth(opacity, depth))
            .collect();

        Self {
            commands,
            bounds: self.bounds,
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
