//! A row's move commands: the controls and menu entries for them and the action
//! that performs them.

use std::rc::Rc;

use flui_foundation::ViewKey;
use flui_foundation::geometry::Axis;

use flui_platform::input::keyboard::NamedKey;
use flui_view::EventContext;

use crate::interaction::{Action, ActionOutcome, GestureCommand, SingleActivator};
use crate::localization::WidgetsLocalizations;

use super::config::{Config, Layout};
use super::session::Shared;
use super::steps::{GridShape, move_target};
use super::types::{ReorderCause, ReorderIntent};

/// The glyph a visible control for `intent` shows.
pub(super) fn arrow(intent: ReorderIntent) -> &'static str {
    match intent {
        ReorderIntent::Start => "\u{2912}",
        ReorderIntent::Up => "\u{2191}",
        ReorderIntent::Down => "\u{2193}",
        ReorderIntent::End => "\u{2913}",
        ReorderIntent::Left => "\u{2190}",
        ReorderIntent::Right => "\u{2192}",
    }
}

/// Every move of a row of `view` (laid out as `shape` when it is a grid), in
/// menu order, with whether it applies to the row at `index`: a list's moves
/// along it, a grid's along its rows and across them.
pub(super) fn moves(
    strings: &dyn WidgetsLocalizations,
    view: &Config,
    shape: Option<GridShape>,
    index: usize,
    rtl: bool,
) -> Vec<(GestureCommand, ReorderIntent, SingleActivator, bool)> {
    let up = (
        strings.reorder_item_up(),
        ReorderIntent::Up,
        NamedKey::ArrowUp,
    );
    let down = (
        strings.reorder_item_down(),
        ReorderIntent::Down,
        NamedKey::ArrowDown,
    );
    let left = (
        strings.reorder_item_left(),
        ReorderIntent::Left,
        NamedKey::ArrowLeft,
    );
    let right = (
        strings.reorder_item_right(),
        ReorderIntent::Right,
        NamedKey::ArrowRight,
    );
    let steps = match (&view.layout, view.scroll_direction, rtl) {
        (Layout::List { .. }, Axis::Vertical, _) => vec![up, down],
        (Layout::List { .. }, Axis::Horizontal, false) => vec![left, right],
        (Layout::List { .. }, Axis::Horizontal, true) => vec![right, left],
        (Layout::Grid { .. }, _, _) => vec![up, down, left, right],
    };
    let start = (
        strings.reorder_item_to_start(),
        ReorderIntent::Start,
        NamedKey::Home,
    );
    let end = (
        strings.reorder_item_to_end(),
        ReorderIntent::End,
        NamedKey::End,
    );
    std::iter::once(start)
        .chain(steps)
        .chain(std::iter::once(end))
        .map(|(label, intent, named)| {
            (
                GestureCommand::new(label, intent),
                intent,
                SingleActivator::named(named).alt(),
                move_target(view, shape, rtl, index, intent).is_some(),
            )
        })
        .collect::<Vec<_>>()
}

/// Performs a row's moves.
pub(super) struct RowMoves {
    pub(super) list: Rc<Shared>,
    pub(super) key: Rc<dyn ViewKey>,
    pub(super) rtl: bool,
}

impl RowMoves {
    pub(super) fn target(&self, intent: ReorderIntent) -> Option<usize> {
        // The row's index now, not when it was built: the app may have moved
        // its model since, by an earlier command delivered before a rebuild.
        // The key is resolved outside the borrow, since `key_of` is app code.
        // A grid's rows are those of its last layout.
        let view = self.list.view.borrow().clone();
        let index = view.index_of(&*self.key)?;
        move_target(&view, self.list.grid.get(), self.rtl, index, intent)
    }
}

impl Action<ReorderIntent> for RowMoves {
    fn is_enabled(&self, intent: &ReorderIntent) -> bool {
        self.target(*intent).is_some()
    }

    fn invoke(&self, ctx: &mut EventContext<'_>, intent: &ReorderIntent) -> ActionOutcome {
        match self.target(*intent) {
            Some(to)
                if self
                    .list
                    .reorder(ctx, &*self.key, to, ReorderCause::Command) =>
            {
                ActionOutcome::Performed
            }
            _ => ActionOutcome::NotPerformed,
        }
    }
}
