//! Swiped rows: [`Dismissible`] drags its child out of view, [`SwipeActions`]
//! reveals actions at a row's edges. Both are an [`AnchoredDrag`] whose
//! outcomes [`GestureAlternatives`] reach without the swipe.
//!
//! [`AnchoredDrag`]: crate::AnchoredDrag
//! [`GestureAlternatives`]: crate::GestureAlternatives

mod dismissible;
mod row;
mod swipe_actions;

pub use dismissible::{
    DismissDirection, DismissDirectionCallback, DismissThreshold, DismissThresholdError,
    DismissUpdateCallback, DismissUpdateDetails, Dismissible, DismissibleState,
};
pub use swipe_actions::{
    SwipeAction, SwipeActions, SwipeActionsError, SwipeActionsState, SwipeAlternative, SwipeEdge,
};
