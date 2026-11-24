//! The horizontal layout of a navigation bar's `leading`, `middle` and
//! `trailing` slots.
//!
//! `leading` sits at the start edge and `trailing` at the end edge, each at
//! its own width; `middle` is centred on the toolbar's full width when that
//! keeps it clear of both by `middle_spacing`, and otherwise slides toward
//! the centre only as far as the side it would collide with allows, limited
//! to the width between them. Start and end follow the ambient text
//! direction.
//!
//! Flutter reference: `NavigationToolbar` and `_ToolbarLayout`
//! (`flutter/flutter@3.50.1 packages/flutter/lib/src/widgets/navigation_toolbar.dart:26`
//! and `:81`). One difference: Flutter keeps `middleSpacing` from `leading`
//! but lets a centred `middle` come within it of `trailing` (its overflow test
//! at `navigation_toolbar.dart:145` compares against the trailing edge without
//! the spacing); here the spacing holds on both sides.
//!
//! The layout is design-neutral; it lives in this package because the only
//! bar that uses it is Cupertino's. Material's `AppBar` lays out the same
//! three slots and could share it from a lower layer.

use std::any::Any;

use flui_sdk::foundation::TextDirection;
use flui_sdk::geometry::{Offset, Size};
use flui_sdk::rendering::{
    BoxConstraints, MultiChildLayoutContext, MultiChildLayoutDelegate, RenderResult,
};
use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{CustomMultiChildLayout, Directionality, LayoutId};

const SLOT_LEADING: &str = "leading";
const SLOT_MIDDLE: &str = "middle";
const SLOT_TRAILING: &str = "trailing";

/// A toolbar of up to three slots laid out by [`ToolbarLayout`], in the
/// ambient [`Directionality`] (left to right without one).
#[derive(Clone, StatelessView)]
pub(crate) struct NavigationToolbar {
    pub(crate) leading: Option<BoxedView>,
    pub(crate) middle: Option<BoxedView>,
    pub(crate) trailing: Option<BoxedView>,
    /// The horizontal gap, in logical pixels, `middle` keeps from `leading`
    /// and from `trailing`.
    pub(crate) middle_spacing: f64,
}

impl std::fmt::Debug for NavigationToolbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationToolbar")
            .field("has_leading", &self.leading.is_some())
            .field("has_middle", &self.middle.is_some())
            .field("has_trailing", &self.trailing.is_some())
            .field("middle_spacing", &self.middle_spacing)
            .finish()
    }
}

impl StatelessView for NavigationToolbar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let delegate = ToolbarLayout {
            middle_spacing: self.middle_spacing,
            text_direction: Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr),
        };
        let slots = [
            (SLOT_LEADING, &self.leading),
            (SLOT_MIDDLE, &self.middle),
            (SLOT_TRAILING, &self.trailing),
        ];
        let children: Vec<LayoutId> = slots
            .into_iter()
            .filter_map(|(slot, child)| child.clone().map(|child| LayoutId::new(slot, child)))
            .collect();
        CustomMultiChildLayout::new(delegate, children)
    }
}

/// The delegate [`NavigationToolbar`] lays its slots out with.
#[derive(Clone, Debug, PartialEq)]
struct ToolbarLayout {
    middle_spacing: f64,
    text_direction: TextDirection,
}

impl ToolbarLayout {
    /// `x` measured from the start edge, as an offset from the left edge of
    /// a toolbar `toolbar_width` wide for a child `child_width` wide.
    fn start_offset_to_x(&self, x: f64, child_width: f64, toolbar_width: f64) -> f64 {
        match self.text_direction {
            TextDirection::Rtl => toolbar_width - child_width - x,
            TextDirection::Ltr => x,
        }
    }
}

impl MultiChildLayoutDelegate for ToolbarLayout {
    fn perform_layout(
        &self,
        ctx: &mut dyn MultiChildLayoutContext,
        size: Size,
    ) -> RenderResult<()> {
        let mut leading_width = 0.0;
        if ctx.has_child(SLOT_LEADING) {
            // Exactly the toolbar's height, so the slot can centre its content
            // vertically; any width up to the toolbar's.
            let constraints = BoxConstraints::new(0.0, size.width, size.height, size.height);
            leading_width = ctx.layout_child(SLOT_LEADING, constraints)?.width;
            let x = self.start_offset_to_x(0.0, leading_width, size.width);
            ctx.position_child(SLOT_LEADING, Offset::new(x, 0.0));
        }

        let mut trailing_width = 0.0;
        if ctx.has_child(SLOT_TRAILING) {
            let trailing = ctx.layout_child(SLOT_TRAILING, BoxConstraints::loose(size))?;
            trailing_width = trailing.width;
            let x = self.start_offset_to_x(size.width - trailing.width, trailing.width, size.width);
            let y = (size.height - trailing.height) / 2.0;
            ctx.position_child(SLOT_TRAILING, Offset::new(x, y));
        }

        if ctx.has_child(SLOT_MIDDLE) {
            let spacing = self.middle_spacing;
            let max_width = (size.width - leading_width - trailing_width - spacing * 2.0).max(0.0);
            let constraints = BoxConstraints::new(0.0, max_width, 0.0, size.height);
            let middle = ctx.layout_child(SLOT_MIDDLE, constraints)?;

            // The start offsets that keep `spacing` from each side. When the
            // sides leave less than `2 * spacing` between them the latest start
            // is below the earliest, and `middle` keeps to `leading`.
            let earliest = leading_width + spacing;
            let latest = size.width - trailing_width - spacing - middle.width;
            let centred = (size.width - middle.width) / 2.0;
            let start = centred.min(latest).max(earliest);

            let x = self.start_offset_to_x(start, middle.width, size.width);
            let y = (size.height - middle.height) / 2.0;
            ctx.position_child(SLOT_MIDDLE, Offset::new(x, y));
        }
        Ok(())
    }

    fn size(&self, constraints: BoxConstraints) -> Size {
        // The bar gives the toolbar a bounded box; an unbounded axis takes its
        // minimum rather than publishing an infinite size.
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let height = if constraints.has_bounded_height() {
            constraints.max_height
        } else {
            constraints.min_height
        };
        Size::new(width, height)
    }

    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        // Every field is a layout input.
        (old_delegate as &dyn Any)
            .downcast_ref::<Self>()
            .is_none_or(|old| self != old)
    }
}
