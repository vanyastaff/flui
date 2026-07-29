//! [`Flex`], [`Row`], and [`Column`] — lay out children along an axis.

use std::fmt;

use flui_objects::{
    CrossAxisAlignment, FlexDirection, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use flui_rendering::protocol::BoxProtocol;
use flui_types::typography::TextBaseline;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Shared main/cross-axis configuration for the flex family, with Flutter's
/// defaults (`MainAxisAlignment::Start`, `CrossAxisAlignment::Center`,
/// `MainAxisSize::Max`, `spacing: 0.0`).
///
/// `text_baseline` is FLUI's own default rather than a ported one: Flutter's
/// `textBaseline` is nullable and required only under
/// `CrossAxisAlignment.baseline`, which it enforces by throwing. FLUI carries a
/// plain [`TextBaseline`], so it defaults to `Alphabetic` — the value that
/// makes the required case work and that every other alignment ignores.
#[derive(Clone, Copy, Debug)]
struct FlexStyle {
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
    spacing: f32,
    text_baseline: TextBaseline,
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
            spacing: 0.0,
            text_baseline: TextBaseline::Alphabetic,
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
            .with_spacing(self.spacing)
            .with_text_baseline(self.text_baseline)
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

        /// Which baseline to align children on under
        /// [`CrossAxisAlignment::Baseline`]; ignored under every other cross
        /// alignment. Defaults to [`TextBaseline::Alphabetic`].
        #[must_use]
        pub fn text_baseline(mut self, baseline: TextBaseline) -> Self {
            self.style.text_baseline = baseline;
            self
        }

        /// How much space to place between children on the main axis.
        ///
        /// Flutter parity: `widgets/basic.dart` `Flex.spacing` /
        /// `RenderFlex.spacing` (`rendering/flex.dart`), tag `3.44.0` — applied
        /// strictly *between* children (never before the first or after the
        /// last), regardless of [`MainAxisAlignment`]. Defaults to `0.0`.
        #[must_use]
        pub fn spacing(mut self, spacing: f32) -> Self {
            self.style.spacing = spacing;
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

#[cfg(test)]
mod tests {
    //! `RenderFlex` exposes no public `spacing` getter, so wiring is verified
    //! through its derived `Debug` output — the same technique
    //! `crate::wrap::Wrap`'s own builder-wiring tests use for `RenderWrap`.

    use flui_view::RenderView;

    use super::*;

    #[test]
    fn row_spacing_defaults_to_zero() {
        let row: Row = Row::new(Vec::new());
        let render_object = row.create_render_object(&flui_view::RenderObjectContext::detached());
        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("spacing: 0.0"),
            "Row's default spacing must be 0.0, got: {debug}"
        );
    }

    #[test]
    fn row_spacing_builder_reaches_render_flex() {
        let row: Row = Row::new(Vec::new()).spacing(8.0);
        let render_object = row.create_render_object(&flui_view::RenderObjectContext::detached());
        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("spacing: 8.0"),
            "Row::spacing(8.0) must reach RenderFlex, got: {debug}"
        );
    }

    /// Red-check: dropping the `.with_spacing(self.spacing)` call from
    /// `FlexStyle::build` (which both `create_render_object` and
    /// `update_render_object` call) leaves this stale at `spacing: 1.0` and
    /// fails.
    #[test]
    fn row_update_render_object_pushes_updated_spacing() {
        let initial: Row = Row::new(Vec::new()).spacing(1.0);
        let mut render_object =
            initial.create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(format!("{render_object:?}").contains("spacing: 1.0"));

        let updated: Row = Row::new(Vec::new()).spacing(9.0);
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("spacing: 9.0"),
            "update_render_object must push the updated spacing, got: {debug}"
        );
        assert!(
            !debug.contains("spacing: 1.0"),
            "update_render_object must not retain the pre-update spacing, got: {debug}"
        );
    }

    /// `spacing` is generated by the same `flex_style_builders!` expansion for
    /// every flex-family widget — one confirming case for `Column` guards
    /// against a future macro split silently dropping the knob from one side.
    #[test]
    fn column_spacing_builder_reaches_render_flex() {
        let column: Column = Column::new(Vec::new()).spacing(6.0);
        let render_object =
            column.create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(
            format!("{render_object:?}").contains("spacing: 6.0"),
            "Column::spacing(6.0) must reach RenderFlex via the shared macro"
        );
    }
}
