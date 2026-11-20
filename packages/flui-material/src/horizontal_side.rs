//! The physical side of the screen a page-shell slot named by a reading edge
//! sits on.
//!
//! `Scaffold`'s floating action button sits at the reading end, a drawer opens
//! from the reading start or end: each resolves that edge against the ambient
//! `Directionality` once, in `build`, and takes its geometry from the
//! [`HorizontalSide`] — the alignment it hugs, the safe-area inset it clears,
//! the direction pointing into the screen. Layout and gestures never see a
//! reading edge, so they cannot resolve it twice and disagree.

use flui_sdk::geometry::EdgeInsets;
use flui_sdk::painting::{Alignment, TextDirection};

/// The left or right side of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HorizontalSide {
    Left,
    Right,
}

impl HorizontalSide {
    /// The side the reading direction starts on.
    pub(crate) fn start(direction: TextDirection) -> Self {
        match direction {
            TextDirection::Ltr => Self::Left,
            TextDirection::Rtl => Self::Right,
        }
    }

    /// The side the reading direction ends on.
    pub(crate) fn end(direction: TextDirection) -> Self {
        Self::start(direction).opposite()
    }

    fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// The horizontal sign pointing from this side into the screen: `1.0`
    /// for the left side, `-1.0` for the right.
    pub(crate) fn inward(self) -> f64 {
        match self {
            Self::Left => 1.0,
            Self::Right => -1.0,
        }
    }

    /// This side's component of `insets`.
    pub(crate) fn inset(self, insets: &EdgeInsets) -> f64 {
        match self {
            Self::Left => insets.left,
            Self::Right => insets.right,
        }
    }

    /// The alignment that centers a child vertically against this side.
    pub(crate) fn center_alignment(self) -> Alignment {
        match self {
            Self::Left => Alignment::CENTER_LEFT,
            Self::Right => Alignment::CENTER_RIGHT,
        }
    }
}
