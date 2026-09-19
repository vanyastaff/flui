//! `DisplayList` — the recorded sequence of drawing commands a
//! [`Canvas`](crate::Canvas) produces and the engine replays.
//!
//! - [`command`] — the `DrawCommand` enum, the wire vocabulary shared with
//!   `flui-engine`.
//! - [`command_ops`] — `DrawCommand::bounds` and its per-variant geometry.

use std::ops::Index;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_types::geometry::{Pixels, Rect};

pub mod command;
pub mod command_ops;

pub use command::{DrawCommand, DrawOp};
// The paint vocabulary the commands carry; defined in `flui_types::painting`.
pub(crate) use flui_types::painting::{
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
    pub(crate) bounds: Option<Rect<Pixels>>,
}

/// Folds one command's bounds into an accumulating union: the one place that
/// decides whether a rect *seeds* the union or *joins* it, shared by
/// [`DisplayList::push`] and [`DisplayList::append`].
fn accumulate_bounds(acc: &mut Option<Rect<Pixels>>, cmd_bounds: Rect<Pixels>) {
    *acc = Some(match *acc {
        Some(current) => current.union(&cmd_bounds),
        None => cmd_bounds,
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
    pub fn bounds(&self) -> Option<Rect<Pixels>> {
        self.bounds
    }

    pub(crate) fn push(&mut self, command: DrawCommand) {
        if let Some(cmd_bounds) = command.bounds() {
            accumulate_bounds(&mut self.bounds, cmd_bounds);
        }
        self.commands.push(command);
    }

    pub(crate) fn clear(&mut self) {
        self.commands.clear();
        self.bounds = None;
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
        } else if !other.commands.is_empty() {
            self.commands.append(&mut other.commands);
            if let Some(other_bounds) = other.bounds {
                accumulate_bounds(&mut self.bounds, other_bounds);
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
