//! The values an app sees: a move ([`Reorder`]), what caused it, the live preview of
//! a drag, the handle policy, the move commands ([`ReorderIntent`]) and the item
//! a builder is asked to show.

#[cfg(doc)]
use super::{ReorderableGrid, ReorderableList};
use std::fmt;
use std::hash::Hash;

use flui_foundation::ViewKey;

use flui_view::prelude::*;

use crate::interaction::Intent;

use super::row::HandleSource;

/// An item's move, as [`ReorderableList`] or [`ReorderableGrid`] reports it
/// once: the item at `from` goes to `to`.
///
/// `to` is the item's index **after** it is removed from `from`, so the move
/// is exactly a `Vec::remove` then a `Vec::insert`:
///
/// ```rust
/// use flui_widgets::Reorder;
///
/// fn apply<T>(items: &mut Vec<T>, change: Reorder) {
///     let item = items.remove(change.from);
///     items.insert(change.to, item);
/// }
///
/// // Dragging "a" onto "d" puts it where "d" was: last.
/// let mut items = vec!["a", "b", "c", "d"];
/// let item = items.remove(0);
/// items.insert(3, item);
/// assert_eq!(items, ["b", "c", "d", "a"]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Reorder {
    /// Where the item was.
    pub from: usize,
    /// Where it goes, counted after it left `from`.
    pub to: usize,
    /// What moved it.
    pub cause: ReorderCause,
}

/// What moved an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderCause {
    /// A drag of the item, dropped on another item.
    Drag,
    /// A move command: its keys, assistive technology's action or the menu.
    Command,
}

/// Where a drag of an item is while it runs, for a list or grid that shows
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ReorderPreview {
    /// Where the dragged item is.
    pub from: usize,
    /// Where a drop now would move the item: the position of the item the
    /// drag is over, counted in the order the list shows while it makes
    /// room; `None` once the drag left the list.
    pub over: Option<usize>,
}

/// Where an item's drag handle comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderHandles {
    /// The list puts a handle before each row's content, and the grid one
    /// above each tile's.
    #[default]
    Default,
    /// Each item places its own, through [`ReorderableItem::handle`].
    Custom,
}

/// One move of a reorderable item, for the platform's keys, assistive
/// technology and the item's menu.
///
/// The arrows name the direction the item moves on screen. In a list they
/// move it one position; in a grid the arrows along the grid's rows move it
/// one position too (from a row's end to the next row's start), and the
/// arrows across them by a whole row, onto the last tile when the next row
/// is short.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderIntent {
    /// To the first position.
    Start,
    /// Up: one position back in a vertical list, a row up in a vertical grid,
    /// one tile up in a horizontal grid.
    Up,
    /// Down: one position forward in a vertical list, a row down in a
    /// vertical grid, one tile down in a horizontal grid.
    Down,
    /// To the last position.
    End,
    /// Left: one position in a horizontal list or one tile in a vertical
    /// grid, a row in a horizontal grid; back in a left-to-right locale,
    /// forward in a right-to-left one.
    Left,
    /// Right: the opposite of [`Left`](Self::Left).
    Right,
}

impl Intent for ReorderIntent {}

/// What the item builder of [`ReorderableList`] or [`ReorderableGrid`] knows
/// of the item it builds.
pub struct ReorderableItem<'a> {
    pub(super) index: usize,
    pub(super) key: &'a dyn ViewKey,
    pub(super) handle: &'a HandleSource,
}

impl ReorderableItem<'_> {
    /// The item's index.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// The item's model key.
    #[must_use]
    pub fn key(&self) -> &dyn ViewKey {
        self.key
    }

    /// `child` as the item's drag handle: a press on it that moves drags the
    /// item, and a tap opens the item's menu. Place one in each item of a
    /// list or grid with [`ReorderHandles::Custom`].
    #[must_use]
    pub fn handle(&self, child: impl IntoView) -> BoxedView {
        self.handle.handle(child.into_view().boxed())
    }
}

impl fmt::Debug for ReorderableItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReorderableItem")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}
