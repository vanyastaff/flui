//! Sealed extension-trait pair: `DisplayListCore` + `DisplayListExt`.
//!
//! Mythos chain U5 extracted these from the 2,434-LOC
//! `display_list.rs` god module. The sealed-extension-trait pattern is
//! the legitimate cross-crate seam consumed by `flui-layer` and
//! `flui-engine`:
//!
//! - `DisplayListCore` is the minimal interface every type
//!   representing a display list exposes (`commands()`, `bounds()`,
//!   `len()`, `is_empty()`).
//! - `DisplayListExt` is a blanket-implemented superset of helpers
//!   (filter iterators, count stats) auto-applied to anything
//!   implementing `DisplayListCore`.
//! - 4 blanket impls: `DisplayList`, `Arc<DisplayList>`,
//!   `Box<DisplayList>`, `&DisplayList`. The `Arc<DisplayList>` impl
//!   is the load-bearing one for `flui-layer::Layer::Picture`
//!   retained-layer caching.
//!
//! Nested wrappers (`Arc<Box<DisplayList>>`, `Box<Arc<DisplayList>>`,
//! `Rc<...>`) are *not* in the impl set. Callers that need a doubly
//! wrapped display list should `.as_ref()` down to the underlying
//! `DisplayList` (or to one of the four supported wrappers) before
//! invoking `DisplayListCore` / `DisplayListExt` methods.
//!
//! Sealing prevents external `impl DisplayListCore for MyType` while
//! preserving the ability to add methods to `DisplayListExt` without
//! breaking changes.

use std::sync::Arc;

use flui_types::geometry::{Pixels, Rect};

use super::command::DrawCommand;
use super::stats::DisplayListStats;
use crate::display_list::DisplayList;

/// Internal module for sealing traits.
///
/// This module is public for technical reasons (trait bounds) but
/// should not be used directly. It is hidden from documentation.
#[doc(hidden)]
pub mod private {
    /// Sealed trait to prevent external implementations of
    /// `DisplayListCore`.
    pub trait Sealed {}
}

/// Core `DisplayList` API providing fundamental access methods.
///
/// This trait defines the minimal interface for accessing display list
/// data. It is sealed to prevent external implementations, allowing
/// the library to add methods to [`DisplayListExt`] in the future
/// without breaking changes.
///
/// # Implementation
///
/// This trait is automatically implemented for:
/// - [`DisplayList`]
/// - `Arc<DisplayList>`
/// - `Box<DisplayList>`
/// - `&DisplayList`
///
/// All implementors automatically receive extension methods from
/// [`DisplayListExt`] via a blanket implementation.
pub trait DisplayListCore: private::Sealed {
    /// Returns an iterator over all commands in this display list.
    fn commands(&self) -> impl Iterator<Item = &DrawCommand>;

    /// Returns the bounding rectangle containing all drawing
    /// operations.
    fn bounds(&self) -> Rect<Pixels>;

    /// Returns the total number of commands in this display list.
    fn len(&self) -> usize;

    /// Returns `true` if this display list contains no commands.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Extension trait providing convenient filtering and query methods.
///
/// Automatically implemented for all types implementing
/// [`DisplayListCore`] via a blanket implementation. Provides
/// zero-cost methods for filtering, counting, and analyzing drawing
/// commands.
///
/// # Performance
///
/// All filtering methods return lazy iterators that don't allocate.
pub trait DisplayListExt: DisplayListCore {
    /// Returns an iterator over drawing commands only.
    ///
    /// Excludes clipping, layer, and effect commands.
    fn draw_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_draw())
    }

    /// Returns an iterator over clipping commands only.
    fn clip_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_clip())
    }

    /// Returns an iterator over shape drawing commands.
    fn shape_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_shape())
    }

    /// Returns an iterator over image and texture commands.
    fn image_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_image())
    }

    /// Returns an iterator over text rendering commands.
    fn text_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|cmd| cmd.is_text())
    }

    /// Counts commands grouped by their category.
    ///
    /// Returns a tuple of `(draw, clip, effect, layer)` command
    /// counts.
    fn count_by_kind(&self) -> (usize, usize, usize, usize) {
        let mut draw = 0;
        let mut clip = 0;
        let mut effect = 0;
        let mut layer = 0;

        for cmd in self.commands() {
            match cmd.kind() {
                super::command::CommandKind::Draw => draw += 1,
                super::command::CommandKind::Clip => clip += 1,
                super::command::CommandKind::Effect => effect += 1,
                super::command::CommandKind::Layer => layer += 1,
            }
        }

        (draw, clip, effect, layer)
    }

    /// Computes comprehensive statistics about this display list.
    fn stats(&self) -> DisplayListStats {
        let (draw, clip, effect, layer) = self.count_by_kind();
        let shapes = self.commands().filter(|c| c.is_shape()).count();
        let images = self.commands().filter(|c| c.is_image()).count();
        let text = self.commands().filter(|c| c.is_text()).count();

        DisplayListStats {
            total: self.len(),
            draw,
            clip,
            effect,
            layer,
            shapes,
            images,
            text,
            hit_regions: 0, // Will be overridden in DisplayList impl
        }
    }
}

// Blanket implementation: all DisplayListCore types get
// DisplayListExt methods.
impl<T: DisplayListCore> DisplayListExt for T {}

// ===== Sealed + Core impls =====

impl private::Sealed for DisplayList {}

impl DisplayListCore for DisplayList {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands.iter()
    }

    fn bounds(&self) -> Rect<Pixels> {
        self.bounds
    }

    fn len(&self) -> usize {
        self.commands.len()
    }
}

impl private::Sealed for Arc<DisplayList> {}

impl DisplayListCore for Arc<DisplayList> {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        (**self).commands()
    }

    fn bounds(&self) -> Rect<Pixels> {
        (**self).bounds()
    }

    fn len(&self) -> usize {
        (**self).len()
    }
}

impl private::Sealed for Box<DisplayList> {}

impl DisplayListCore for Box<DisplayList> {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        (**self).commands()
    }

    fn bounds(&self) -> Rect<Pixels> {
        (**self).bounds()
    }

    fn len(&self) -> usize {
        (**self).len()
    }
}

impl private::Sealed for &DisplayList {}

impl DisplayListCore for &DisplayList {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand> {
        (*self).commands()
    }

    fn bounds(&self) -> Rect<Pixels> {
        (*self).bounds()
    }

    fn len(&self) -> usize {
        (*self).len()
    }
}
