//! `DisplayList` — the recorded sequence of drawing commands a
//! [`Canvas`](crate::Canvas) produces and the engine replays.
//!
//! - [`command`] — the `DrawCommand` enum, the wire vocabulary shared with
//!   `flui-engine`.
//! - [`command_ops`] — `DrawCommand::bounds` and its per-variant geometry.

use std::ops::Index;

use flui_foundation::geometry::Rect;
use flui_foundation::{Diagnosticable, DiagnosticsBuilder};

pub mod command;
pub mod command_ops;

pub use command::{DrawCommand, DrawOp};
pub use command_ops::DamageExtent;
// The paint vocabulary the commands carry; defined in `crate::paint`.
pub(crate) use crate::paint::{
    BlendMode, Clip, ClipOp, FilterQuality, Paint, PointMode, TextureId,
    image::{ColorFilter, ImageRepeat},
};

/// A recorded sequence of drawing commands.
///
/// The output of [`Canvas::finish`](crate::Canvas::finish) and the input to
/// `PictureLayer`. Commands are read-only; recording, concatenation, and
/// deserialization compute the cached [`bounds`](Self::bounds) from command geometry.
#[derive(Debug, Clone)]
pub struct DisplayList {
    /// Drawing commands in order.
    pub(crate) commands: Vec<DrawCommand>,

    /// Cached union of every command that contributes bounds, or
    /// `None` while no command has contributed one yet.
    ///
    /// `None` and "an empty rect" are different states, and collapsing
    /// them is a bug: a list whose first bounds-carrying command sits at
    /// (100, 100) must not report a rect reaching back to the origin just
    /// because a clip or a `Save` was recorded ahead of it. Seeding is
    /// therefore keyed on this being `None` — never on `commands` being
    /// empty, which asks a different question, and never on the rect
    /// comparing equal to `Rect::ZERO`, which a degenerate command can
    /// legitimately produce.
    pub(crate) bounds: Option<Rect<f64>>,

    /// Cached conservative extent of every pixel the list can change, or
    /// `None` while no command has drawn anything — see
    /// [`Self::damage_extent`].
    ///
    /// Kept apart from `bounds` because the two answer different questions:
    /// `bounds` is the layout-facing box (a paragraph's laid-out size, no
    /// entry for a full-canvas fill), while this one must never fall short of
    /// the ink, since a damage region computed from it decides which pixels a
    /// partial repaint may leave untouched.
    pub(crate) damage: Option<DamageExtent>,

    /// Cached extent of every command whose pixels come from outside the
    /// list, or `None` while no such command was recorded — see
    /// [`Self::volatile_extent`].
    pub(crate) volatile: Option<DamageExtent>,
}

/// Folds one command's bounds into an accumulating union: the one place that
/// decides whether a rect *seeds* the union or *joins* it, shared by
/// [`DisplayList::push`] and [`DisplayList::append`].
fn accumulate_bounds(acc: &mut Option<Rect<f64>>, cmd_bounds: Rect<f64>) {
    *acc = Some(match *acc {
        Some(current) => current.union(&cmd_bounds),
        None => cmd_bounds,
    });
}

/// Folds one command's damage extent into an accumulating one; the
/// counterpart of [`accumulate_bounds`] for [`DisplayList::damage_extent`].
fn accumulate_damage(acc: &mut Option<DamageExtent>, extent: DamageExtent) {
    *acc = Some(match *acc {
        Some(current) => current.union(extent),
        None => extent,
    });
}

impl Diagnosticable for DisplayList {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("commands", self.commands.len());
        properties.add("bounds", format!("{:?}", self.bounds));
    }
}

impl DisplayList {
    /// An empty list.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            bounds: None,
            damage: None,
            volatile: None,
        }
    }

    /// The commands, in recording order.
    pub fn iter(&self) -> std::slice::Iter<'_, DrawCommand> {
        self.commands.iter()
    }

    /// The commands as a slice.
    #[must_use]
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// The number of commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether no command was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The union of every command that contributes bounds, or `None` when
    /// none has — a list of only clips, saves, or `DrawPaint`s has no extent,
    /// which is a different answer from an empty rect at the origin.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect<f64>> {
        self.bounds
    }

    /// A conservative extent of every pixel replaying this list can change,
    /// in the list's own coordinate space; `None` when no command draws.
    ///
    /// Unlike [`Self::bounds`], this never falls short of the ink: a paragraph
    /// contributes its glyph overflow as well as its laid-out box, a shadow
    /// its blur, a stroke its joins, and a full-canvas fill (`drawColor`,
    /// `drawPaint`, an unbounded save-layer) makes the extent
    /// [`DamageExtent::Unbounded`]. Clips are ignored, which only ever makes
    /// the answer larger. This is what a damage region is built from: a rect
    /// that misses a pixel the list changed leaves that pixel stale.
    #[must_use]
    pub fn damage_extent(&self) -> Option<DamageExtent> {
        self.damage
    }

    /// The extent of the commands whose pixels this list does not
    /// determine, in the list's own coordinate space; `None` when there is
    /// none.
    ///
    /// A texture draw (`DrawOp::Texture`) names an external texture whose
    /// content its producer (a video decoder, a camera, a platform surface)
    /// replaces behind the same id, so replaying an unchanged list can paint
    /// different pixels there. A damage producer that vouches for unchanged
    /// content by the list's identity treats this extent as changed on
    /// every frame.
    #[must_use]
    pub fn volatile_extent(&self) -> Option<DamageExtent> {
        self.volatile
    }

    pub(crate) fn push(&mut self, command: DrawCommand) {
        if let Some(cmd_bounds) = command.bounds() {
            accumulate_bounds(&mut self.bounds, cmd_bounds);
        }
        if let Some(extent) = command.damage_extent() {
            accumulate_damage(&mut self.damage, extent);
            if matches!(command.op, DrawOp::Texture { .. }) {
                accumulate_damage(&mut self.volatile, extent);
            }
        }
        self.commands.push(command);
    }

    pub(crate) fn clear(&mut self) {
        self.commands.clear();
        self.bounds = None;
        self.damage = None;
        self.volatile = None;
    }

    /// Moves every command of `other` onto the end of this list, unioning
    /// the cached bounds.
    ///
    /// Concatenates replay state too: a root clip in this list affects `other`,
    /// even when both lists have balanced save/restore pairs. Use
    /// [`Self::append_isolated`] for each independently recorded paint run
    /// when building an accumulator.
    pub fn append(&mut self, mut other: DisplayList) {
        if self.commands.is_empty() {
            std::mem::swap(&mut self.commands, &mut other.commands);
            self.bounds = other.bounds;
            self.damage = other.damage;
            self.volatile = other.volatile;
        } else if !other.commands.is_empty() {
            self.commands.append(&mut other.commands);
            if let Some(other_bounds) = other.bounds {
                accumulate_bounds(&mut self.bounds, other_bounds);
            }
            if let Some(other_damage) = other.damage {
                accumulate_damage(&mut self.damage, other_damage);
            }
            if let Some(other_volatile) = other.volatile {
                accumulate_damage(&mut self.volatile, other_volatile);
            }
        }
    }

    /// Appends a balanced paint run inside its own save/restore scope.
    ///
    /// The run inherits the caller's clip state, but its clips do not affect
    /// subsequent commands. An empty run records nothing. The caller must
    /// provide balanced save/restore commands within the run.
    pub fn append_isolated(&mut self, other: DisplayList) {
        if other.is_empty() {
            return;
        }
        self.push(DrawCommand::untransformed(DrawOp::Save));
        self.append(other);
        self.push(DrawCommand::untransformed(DrawOp::Restore));
    }
}

impl Default for DisplayList {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<[DrawCommand]> for DisplayList {
    fn as_ref(&self) -> &[DrawCommand] {
        &self.commands
    }
}

impl<'a> IntoIterator for &'a DisplayList {
    type Item = &'a DrawCommand;
    type IntoIter = std::slice::Iter<'a, DrawCommand>;

    fn into_iter(self) -> Self::IntoIter {
        self.commands.iter()
    }
}

impl Index<usize> for DisplayList {
    type Output = DrawCommand;

    fn index(&self, index: usize) -> &Self::Output {
        &self.commands[index]
    }
}
