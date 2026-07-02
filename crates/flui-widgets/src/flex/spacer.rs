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

#[cfg(test)]
mod tests {
    use super::*;

    // `tests/spacer.rs` already proves the `build()` composition
    // (`Expanded::new(SizedBox::shrink()).flex(flex)`) reaches the render
    // tree with correct flex-splitting/offset behavior via full layout.
    // These tests cover what that integration coverage can't reach: the
    // bare builder/`Default` surface and the manual `Debug` impl, both
    // exercised directly via the private `flex` field (visible here since
    // `tests` is a descendant of `spacer`'s defining module).

    #[test]
    fn default_flex_factor_is_one() {
        assert_eq!(Spacer::default().flex, 1);
    }

    #[test]
    fn new_flex_factor_is_one() {
        assert_eq!(Spacer::new().flex, 1);
    }

    #[test]
    fn flex_builder_overrides_the_default_flex_factor() {
        assert_eq!(Spacer::new().flex(5).flex, 5);
    }

    #[test]
    fn flex_builder_accepts_zero() {
        assert_eq!(Spacer::new().flex(0).flex, 0);
    }

    #[test]
    fn flex_builder_accepts_negative_values() {
        assert_eq!(Spacer::new().flex(-3).flex, -3);
    }

    #[test]
    fn flex_builder_last_call_wins() {
        assert_eq!(Spacer::new().flex(3).flex(7).flex, 7);
    }

    #[test]
    fn debug_format_reports_the_default_flex_factor() {
        assert_eq!(format!("{:?}", Spacer::new()), "Spacer { flex: 1 }");
    }

    #[test]
    fn debug_format_reports_an_overridden_flex_factor() {
        assert_eq!(format!("{:?}", Spacer::new().flex(4)), "Spacer { flex: 4 }");
    }

    #[test]
    fn clone_preserves_the_flex_factor() {
        let original = Spacer::new().flex(9);
        let cloned = original.clone();
        assert_eq!(cloned.flex, 9);
    }
}
