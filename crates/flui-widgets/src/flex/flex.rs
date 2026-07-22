//! [`Flex`], [`Row`], and [`Column`] — lay out children along an axis.

use std::fmt;

use flui_objects::{
    CrossAxisAlignment, FlexDirection, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use flui_rendering::protocol::BoxProtocol;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Shared main/cross-axis configuration for the flex family, with Flutter's
/// defaults (`MainAxisAlignment::Start`, `CrossAxisAlignment::Center`,
/// `MainAxisSize::Max`).
#[derive(Clone, Copy, Debug)]
struct FlexStyle {
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
        }
    }
}

impl FlexStyle {
    fn build(self, direction: FlexDirection) -> RenderFlex {
        let base = match direction {
            FlexDirection::Horizontal => RenderFlex::row(),
            FlexDirection::Vertical => RenderFlex::column(),
        };
        base.with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_main_axis_size(self.main_axis_size)
    }
}

/// Generate the shared builder methods (main/cross alignment, main-axis size)
/// for a flex-family widget that stores its config in a `style: FlexStyle`.
macro_rules! flex_style_builders {
    () => {
        /// How children are placed along the main axis.
        #[must_use]
        pub fn main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
            self.style.main_axis_alignment = alignment;
            self
        }

        /// How children are placed along the cross axis.
        #[must_use]
        pub fn cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
            self.style.cross_axis_alignment = alignment;
            self
        }

        /// Whether the main axis shrink-wraps children (`Min`) or fills the
        /// available extent (`Max`, the default).
        #[must_use]
        pub fn main_axis_size(mut self, size: MainAxisSize) -> Self {
            self.style.main_axis_size = size;
            self
        }
    };
}

/// Lays out children along a configurable [`FlexDirection`].
///
/// Flutter parity: `widgets/basic.dart` `Flex` over `RenderFlex`. Prefer
/// [`Row`] / [`Column`] for the common fixed-direction cases.
///
/// Generic over `C: ViewSeq`: a static `column!`/`row!` tuple keeps each child
/// monomorphic (the contract-C2 fast path), while a `Vec<BoxedView>` carries a
/// dynamic, runtime-sized child list.
#[derive(Clone)]
pub struct Flex<C = Vec<BoxedView>> {
    direction: FlexDirection,
    style: FlexStyle,
    children: C,
}

impl<C> Flex<C> {
    /// A flex laid out along `direction` with the given children.
    pub fn new(direction: FlexDirection, children: C) -> Self {
        Self {
            direction,
            style: FlexStyle::default(),
            children,
        }
    }

    flex_style_builders!();
}

impl<C: ViewSeq> fmt::Debug for Flex<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Flex")
            .field("direction", &self.direction)
            .field("style", &self.style)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for Flex<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.style.build(self.direction)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        *render_object = self.style.build(self.direction);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Flex);

/// Lays out children horizontally (Flutter's `Row`).
#[derive(Clone)]
pub struct Row<C = Vec<BoxedView>> {
    style: FlexStyle,
    children: C,
}

impl<C> Row<C> {
    /// A horizontal row of the given children.
    pub fn new(children: C) -> Self {
        Self {
            style: FlexStyle::default(),
            children,
        }
    }

    flex_style_builders!();
}

impl<C: ViewSeq> fmt::Debug for Row<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Row")
            .field("style", &self.style)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for Row<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.style.build(FlexDirection::Horizontal)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        *render_object = self.style.build(FlexDirection::Horizontal);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Row);

/// Lays out children vertically (Flutter's `Column`).
#[derive(Clone)]
pub struct Column<C = Vec<BoxedView>> {
    style: FlexStyle,
    children: C,
}

impl<C> Column<C> {
    /// A vertical column of the given children.
    pub fn new(children: C) -> Self {
        Self {
            style: FlexStyle::default(),
            children,
        }
    }

    flex_style_builders!();
}

impl<C: ViewSeq> fmt::Debug for Column<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Column")
            .field("style", &self.style)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for Column<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.style.build(FlexDirection::Vertical)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        *render_object = self.style.build(FlexDirection::Vertical);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Column);
