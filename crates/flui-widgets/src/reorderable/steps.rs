//! Where a move command puts a row: a grid's shape at its last layout and the step
//! each intent takes along a list or a grid.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::geometry::Axis;
use flui_rendering::constraints::SliverConstraints;
use flui_rendering::delegates::{SliverGridDelegate, SliverGridLayout};

use super::config::{Config, Layout};
use super::types::ReorderIntent;

/// How a grid laid its tiles out at its last layout: what its row moves need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GridShape {
    /// The tiles in one row across the scroll axis.
    pub(super) stride: usize,
    /// Whether the index order runs against the screen along the scroll axis
    /// (a horizontal grid in a right-to-left locale).
    pub(super) main_reversed: bool,
    /// Whether it runs against the screen across it.
    pub(super) cross_reversed: bool,
}

impl GridShape {
    /// The shape assumed before the grid is laid out: no row moves, and the
    /// tiles in the screen's order.
    pub(super) const UNKNOWN: Self = Self {
        stride: 1,
        main_reversed: false,
        cross_reversed: false,
    };
}

/// The app's grid delegate, which also records the shape of each layout it
/// gives, so the row moves go by the rows on screen.
#[derive(Debug)]
pub(super) struct ShapeRecorder {
    pub(super) inner: Rc<dyn SliverGridDelegate>,
    pub(super) shape: Rc<Cell<Option<GridShape>>>,
}

impl SliverGridDelegate for ShapeRecorder {
    fn layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        let layout = self.inner.layout(constraints);
        self.shape.set(Some(GridShape {
            stride: layout.cross_axis_count.max(1),
            main_reversed: constraints.axis_direction.is_reversed(),
            cross_reversed: layout.reverse_cross_axis,
        }));
        layout
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        match (old_delegate as &dyn Any).downcast_ref::<Self>() {
            Some(old) => {
                !Rc::ptr_eq(&self.shape, &old.shape) || self.inner.should_relayout(&*old.inner)
            }
            None => true,
        }
    }
}

/// How far a move goes from an item's index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Toward the first item, by this many.
    Back(usize),
    /// Toward the last item, by this many; past the last, onto it, when the
    /// item is not in the last row.
    Forward(usize),
}

/// The step an arrow's move takes along a list, in the list's reading
/// direction; `None` for Start and End, which go to a place, not by a step.
pub(super) fn list_step(intent: ReorderIntent, rtl: bool) -> Option<Step> {
    match intent {
        ReorderIntent::Start | ReorderIntent::End => None,
        ReorderIntent::Left if rtl => Some(Step::Forward(1)),
        ReorderIntent::Right if rtl => Some(Step::Back(1)),
        ReorderIntent::Up | ReorderIntent::Left => Some(Step::Back(1)),
        ReorderIntent::Down | ReorderIntent::Right => Some(Step::Forward(1)),
    }
}

/// The step an arrow's move of a tile takes in a grid shaped `shape` on the
/// `axis`: the arrows along its rows go to the next tile, those across them
/// by a row, each the way it points on screen. `None` for Start and End, and
/// for a row move in a grid of one tile per row, which would only repeat the
/// move along it.
pub(super) fn grid_step(intent: ReorderIntent, axis: Axis, shape: GridShape) -> Option<Step> {
    let toward = |forward_on_screen: bool, reversed: bool, by: usize| {
        if forward_on_screen == reversed {
            Step::Back(by)
        } else {
            Step::Forward(by)
        }
    };
    let row = |forward_on_screen: bool| {
        (shape.stride > 1).then(|| toward(forward_on_screen, shape.main_reversed, shape.stride))
    };
    let along = |forward_on_screen: bool| Some(toward(forward_on_screen, shape.cross_reversed, 1));
    match (axis, intent) {
        (_, ReorderIntent::Start | ReorderIntent::End) => None,
        (Axis::Vertical, ReorderIntent::Up) | (Axis::Horizontal, ReorderIntent::Left) => row(false),
        (Axis::Vertical, ReorderIntent::Down) | (Axis::Horizontal, ReorderIntent::Right) => {
            row(true)
        }
        (Axis::Vertical, ReorderIntent::Left) | (Axis::Horizontal, ReorderIntent::Up) => {
            along(false)
        }
        (Axis::Vertical, ReorderIntent::Right) | (Axis::Horizontal, ReorderIntent::Down) => {
            along(true)
        }
    }
}

/// Where `intent` moves the item at `index` of `view`, laid out as `shape`
/// when it is a grid; `None` when the move does not apply there.
pub(super) fn move_target(
    view: &Config,
    shape: Option<GridShape>,
    rtl: bool,
    index: usize,
    intent: ReorderIntent,
) -> Option<usize> {
    let last = view.item_count.checked_sub(1)?;
    if index > last {
        return None;
    }
    let target = match intent {
        ReorderIntent::Start => 0,
        ReorderIntent::End => last,
        _ => {
            let step = match view.layout {
                Layout::List { .. } => list_step(intent, rtl),
                Layout::Grid { .. } => grid_step(
                    intent,
                    view.scroll_direction,
                    shape.unwrap_or(GridShape::UNKNOWN),
                ),
            }?;
            match step {
                Step::Back(by) => index.checked_sub(by)?,
                // A row down from a row that is not the last lands on the
                // tile below, or on the last tile when the next row is short.
                Step::Forward(by) => {
                    if index / by >= last / by {
                        return None;
                    }
                    index.saturating_add(by).min(last)
                }
            }
        }
    };
    (target != index).then_some(target)
}
