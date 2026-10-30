//! [`CupertinoPageControl`]: the iOS row of page dots.
//!
//! It is a [`PageIndicator`] in the iOS look: 7 pt dots 9 pt apart, the
//! current page white and the others white at 30 %, as `UIPageControl` draws
//! them over content. Paging, the window over many pages, the announced value
//! and right-to-left order are the indicator's own.

use std::num::NonZeroUsize;

use flui_sdk::animation::Animation;
use flui_sdk::painting::Color;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{PageController, PageIndicator, PageIndicatorStyle};

/// The diameter of a dot, in logical pixels.
const DOT: f64 = 7.0;
/// The space between two dots, in logical pixels.
const GAP: f64 = 9.0;
/// The most dots shown at once.
const WINDOW: usize = 9;

/// The iOS page control: a dot per page, the current one lit.
///
/// [`controlled`](Self::controlled) follows a [`PageController`] and turns its
/// pages on a tap at either side or an assistive-technology step;
/// [`observed`](Self::observed) only shows a position. See [`PageIndicator`]
/// for the behaviour both share.
///
/// # Examples
///
/// ```rust,ignore
/// let pager = PageController::new();
/// CupertinoPageControl::controlled(&pager, 4)
/// ```
#[derive(Debug, Clone, StatelessView)]
pub struct CupertinoPageControl {
    indicator: PageIndicator,
    current: Color,
    other: Color,
}

impl CupertinoPageControl {
    /// A control that follows `controller` and turns its pages.
    #[must_use]
    pub fn controlled(controller: &PageController, count: usize) -> Self {
        Self::wrapping(PageIndicator::controlled(controller, count))
    }

    /// A control that shows `position`, a fractional page, and nothing more.
    #[must_use]
    pub fn observed(position: impl Animation<f64> + 'static, count: usize) -> Self {
        Self::wrapping(PageIndicator::observed(position, count))
    }

    /// The colour of the current page's dot; white by default.
    #[must_use]
    pub fn current_color(mut self, color: Color) -> Self {
        self.current = color;
        self
    }

    /// The colour of the other dots; white at 30 % by default.
    #[must_use]
    pub fn other_color(mut self, color: Color) -> Self {
        self.other = color;
        self
    }

    fn wrapping(indicator: PageIndicator) -> Self {
        Self {
            indicator,
            current: Color::WHITE,
            other: Color::WHITE.with_opacity(0.3),
        }
    }
}

impl StatelessView for CupertinoPageControl {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let style = PageIndicatorStyle::try_new(
            DOT,
            GAP,
            NonZeroUsize::new(WINDOW).expect("BUG: the window is not empty"),
        )
        .expect("BUG: the iOS dot sizes are finite and positive")
        .active_color(self.current)
        .inactive_color(self.other);
        self.indicator.clone().style(style)
    }
}
