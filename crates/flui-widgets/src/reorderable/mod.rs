//! [`ReorderableList`] and [`ReorderableGrid`] — a lazy list and a lazy grid
//! whose items the user moves (ADR-0211 §9).
//!
//! Moving an item is a drag-and-drop session that never leaves the window
//! ([`DragReach::Window`]): an item's handle starts it at once, and on touch
//! and pen a long press on the item starts it too. Every item is a drop target
//! that takes only drags of its own list or grid, and edge autoscroll comes
//! from the scrollable. The dragged item keeps its state while it is scrolled
//! out of the built band: its drag holds the item alive.
//!
//! While a drag runs, the dragged item leaves a gap where a drop would put
//! it, at the item the pointer is over, and the items between make room; what
//! the drag shows lifts from the item's place
//! ([`proxy_decorator`](ReorderableList::proxy_decorator)). A drop or
//! a cancel lands the item in its place from where it was let go, and a move
//! command slides the items the same way: every slide takes 250 ms, and under
//! reduced motion lands on its next frame.
//!
//! The same moves have a path without a drag (ADR-0216): each item carries the
//! commands that apply to it — "Move to the start", "Move to the end" and one
//! step each way, which in a grid are a tile along its row and a whole row —
//! on the platform's keys while focus is in the item, as assistive
//! technology's actions on the item's node, and for one pointer in a context
//! menu opened from the item's handle, a secondary click or the menu key. A
//! command's move is announced with the item's new position.
//!
//! Neither moves anything itself: each calls [`on_reorder`](Reorder) once,
//! and the app moves the item in its model and rebuilds. Items are keyed by
//! the model key they are given, so an item's element, state and focus follow
//! it to the new position.

mod config;
mod grid;
mod list;
mod moves;
mod row;
mod session;
mod steps;
mod types;

#[cfg(doc)]
use flui_interaction::drag::DragReach;
pub use grid::{ReorderableGrid, ReorderableGridState};
pub use list::{ReorderableList, ReorderableListState};
pub use types::{
    Reorder, ReorderCause, ReorderHandles, ReorderIntent, ReorderPreview, ReorderableItem,
};
