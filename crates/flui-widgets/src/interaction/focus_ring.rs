//! [`FocusRing`] — the keyboard focus indicator — and the inherited
//! [`DefaultFocusRingStyle`] that themes it.
//!
//! The ring shows only while the enclosing [`Focus`]
//! has *visible* focus
//! ([`FocusNode::has_visible_focus`](flui_interaction::focus::FocusNode::has_visible_focus)): focus a key,
//! traversal or assistive technology moved or used. Focus a pointer press set
//! is not marked, so a click does not leave a ring behind it.

use std::fmt;

use flui_painting::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_view::impl_inherited_view;
use flui_view::prelude::*;

use super::focus::Focus;
use crate::DecoratedBox;

/// The colors and widths of a [`FocusRing`]: two bands drawn inside the
/// child's bounds, `outer` along the edge and `inner` just within it. A
/// focused link draws the same bands just outside its glyphs.
pub use flui_objects::FocusRingStyle;

/// A uniform `width` border in `color`, or no border for a zero width.
fn ring_band(color: Color, width: f64) -> BoxDecoration<f64> {
    if width > 0.0 {
        BoxDecoration::new().set_border(Some(Border::all(BorderSide::new(
            color,
            width,
            BorderStyle::Solid,
        ))))
    } else {
        BoxDecoration::new()
    }
}

/// Draws the keyboard focus indicator over `child` while the nearest
/// enclosing [`Focus`] has visible focus.
///
/// The ring is painted in front of the child and inside its bounds, so a
/// neighbour drawn later (the next row of a list with no gap) cannot cover
/// it. Its style is `style` when set, else the nearest
/// [`DefaultFocusRingStyle`], else [`FocusRingStyle::BUILT_IN`].
///
/// Place it below the `Focus` it marks:
///
/// ```rust,ignore
/// Focus::new(FocusRing::new(label))
/// ```
///
/// Showing or hiding the ring changes only its paint: the child keeps its
/// elements and state.
#[derive(Clone, StatelessView)]
pub struct FocusRing {
    /// The subtree the ring outlines.
    pub child: BoxedView,
    /// The ring's style; `None` takes the inherited one.
    pub style: Option<FocusRingStyle>,
}

impl FocusRing {
    /// A ring around `child` in the inherited style.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            style: None,
        }
    }

    /// Draw the ring in `style` rather than the inherited one.
    #[must_use]
    pub fn style(mut self, style: FocusRingStyle) -> Self {
        self.style = Some(style);
        self
    }
}

impl fmt::Debug for FocusRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusRing")
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl StatelessView for FocusRing {
    /// The enclosing `Focus` rebuilds its dependents on every change of its
    /// node, a change of input modality included, so this reads the current
    /// visibility each time.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let visible = Focus::maybe_of(ctx).is_some_and(|node| node.has_visible_focus());
        let style = self.style.unwrap_or_else(|| DefaultFocusRingStyle::of(ctx));
        let (outer, inner) = if visible {
            let outer_width = style.outer_band();
            let inner_width = style.inner_band();
            // The inner band is drawn as far in as both bands reach; the
            // outer band is then drawn over its outer part.
            let inner = if inner_width > 0.0 {
                ring_band(style.inner, outer_width + inner_width)
            } else {
                BoxDecoration::new()
            };
            (ring_band(style.outer, outer_width), inner)
        } else {
            (BoxDecoration::new(), BoxDecoration::new())
        };
        // The outer decoration paints after its child, so the outer band lies
        // over the inner band's outer part.
        DecoratedBox::new(outer).foreground().child(
            DecoratedBox::new(inner)
                .foreground()
                .child(self.child.clone()),
        )
    }
}

/// Gives every [`FocusRing`] below it, and every control that draws one, the
/// same ring style — what a design system's theme installs.
#[derive(Clone)]
pub struct DefaultFocusRingStyle {
    style: FocusRingStyle,
    child: BoxedView,
}

impl DefaultFocusRingStyle {
    /// Provide `style` to the rings in `child`.
    #[must_use]
    pub fn new(style: FocusRingStyle, child: impl IntoView) -> Self {
        Self {
            style,
            child: child.into_view().boxed(),
        }
    }

    /// The style rings at `ctx` use: the nearest provided one, else
    /// [`FocusRingStyle::BUILT_IN`]. Registers a dependency, so a ring
    /// rebuilds when the provided style changes.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> FocusRingStyle {
        ctx.depend_on::<Self, _>(|provider| provider.style)
            .unwrap_or_default()
    }
}

impl fmt::Debug for DefaultFocusRingStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultFocusRingStyle")
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl InheritedView for DefaultFocusRingStyle {
    type Data = FocusRingStyle;

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

impl_inherited_view!(DefaultFocusRingStyle);
