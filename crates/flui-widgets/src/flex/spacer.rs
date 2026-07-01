//! [`Spacer`] — flex-proportional empty space in a [`Row`](crate::Row) or [`Column`](crate::Column).

use std::fmt;

use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, IntoView};

use crate::flex::Expanded;
use crate::layout::SizedBox;

/// Creates an empty space proportional to its `flex` factor in a
/// [`Row`](crate::Row) or [`Column`](crate::Column).
///
/// Spacer expands to fill the main-axis share allocated to it, so placing two
/// equal `Spacer`s around a widget centers that widget in the flex container,
/// and using `Spacer`s with different `flex` values distributes remaining space
/// proportionally.
///
/// Implemented as `Expanded::new(SizedBox::shrink()).flex(flex)`, so the cross
/// axis is unconstrained (zero) while the main axis fills the flex share.
///
/// Flutter parity: `widgets/spacer.dart` `Spacer`.
#[derive(Clone, StatelessView)]
pub struct Spacer {
    flex: i32,
}

impl Default for Spacer {
    fn default() -> Self {
        Self { flex: 1 }
    }
}

impl Spacer {
    /// A spacer with flex factor `1`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the flex factor — this spacer's share of the remaining main-axis
    /// space relative to other [`Expanded`]/[`Spacer`] siblings.
    #[must_use]
    pub fn flex(mut self, flex: i32) -> Self {
        self.flex = flex;
        self
    }
}

impl fmt::Debug for Spacer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Spacer").field("flex", &self.flex).finish()
    }
}

impl StatelessView for Spacer {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Expanded::new(SizedBox::shrink()).flex(self.flex)
    }
}
