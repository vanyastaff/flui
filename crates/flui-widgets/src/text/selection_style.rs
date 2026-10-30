//! [`SelectionStyle`]: how selected static text is highlighted, and
//! [`DefaultSelectionStyle`], which sets it for a subtree.

use flui_painting::styling::Color;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, View, ViewExt, impl_inherited_view};

/// How a [`SelectionArea`](crate::SelectionArea) highlights its selection.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_painting::styling::Color;
/// let style = SelectionStyle::new(Color::rgba(0, 120, 215, 96), Color::rgba(128, 128, 128, 64));
/// let _ = SelectionArea::new(Text::new("Terms")).style(style);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SelectionStyle {
    /// The highlight while the area holds the focus.
    pub active: Color,
    /// The dimmed highlight while it does not: a blur keeps the selection.
    pub inactive: Color,
}

impl SelectionStyle {
    /// A style highlighting with `active` while focused and `inactive`
    /// otherwise.
    #[must_use]
    pub const fn new(active: Color, inactive: Color) -> Self {
        Self { active, inactive }
    }
}

impl Default for SelectionStyle {
    /// A translucent blue, dimmed to a translucent grey.
    fn default() -> Self {
        Self::new(
            Color::rgba(51, 153, 255, 102),
            Color::rgba(128, 128, 128, 72),
        )
    }
}

/// Sets the [`SelectionStyle`] of the selection areas beneath it that set
/// none of their own.
#[derive(Clone, Debug)]
pub struct DefaultSelectionStyle {
    style: SelectionStyle,
    child: BoxedView,
}

impl DefaultSelectionStyle {
    /// `style` for the areas in `child`.
    pub fn new(style: SelectionStyle, child: impl IntoView) -> Self {
        Self {
            style,
            child: child.into_view().boxed(),
        }
    }

    /// The style the nearest `DefaultSelectionStyle` above `ctx` sets,
    /// registering a dependency on it.
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<SelectionStyle> {
        ctx.depend_on::<Self, _>(|scope| scope.style)
    }
}

impl InheritedView for DefaultSelectionStyle {
    type Data = SelectionStyle;

    fn data(&self) -> &Self::Data {
        &self.style
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.style != old.style
    }
}

impl_inherited_view!(DefaultSelectionStyle);
