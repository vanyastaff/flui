//! [`Flex`], [`Row`], and [`Column`] — lay out children along an axis.

use std::fmt;

use flui_objects::{
    CrossAxisAlignment, FlexDirection, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use flui_rendering::protocol::BoxProtocol;
use flui_types::typography::TextBaseline;
use flui_types::typography::TextDirection;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::localization::Directionality;
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
    /// `text_direction` is the already-resolved ambient direction — see
    /// [`FlexRenderView`] for why it must be resolved before this call
    /// rather than looked up here.
    fn build(self, direction: FlexDirection, text_direction: TextDirection) -> RenderFlex {
        let base = match direction {
            FlexDirection::Horizontal => RenderFlex::row(),
            FlexDirection::Vertical => RenderFlex::column(),
        };
        base.with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_main_axis_size(self.main_axis_size)
            .with_spacing(self.spacing)
            .with_text_baseline(self.text_baseline)
            .with_text_direction(text_direction)
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
///
/// Resolves the ambient [`Directionality`] the way Flutter's `RenderFlex`
/// does (`rendering/flex.dart`): a horizontal flex (`Row`) consults it for
/// its *main* axis (`Start`/`End` and child order flip under `Rtl`); a
/// vertical flex (`Column`) consults it for its *cross* axis instead — its
/// main axis is governed by `VerticalDirection`, which FLUI does not model.
/// Defaults to [`TextDirection::Ltr`] with no ancestor, matching every other
/// FLUI widget that consults `Directionality`. That ambient read only exists
/// inside a `BuildContext`, so `Flex` is a composing widget: `build` resolves
/// the direction once and hands the *already-resolved* [`TextDirection`] to a
/// private render-object widget — `flui_view::RenderObjectContext` (the only
/// context `RenderView::create_render_object`/`update_render_object` ever
/// receive) carries no ambient-lookup capability, so a bare `RenderView` can
/// never read `Directionality` itself.
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

impl<C> flui_view::View for Flex<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl<C> flui_view::StatelessView for Flex<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        let text_direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        FlexRenderView {
            direction: self.direction,
            style: self.style,
            text_direction,
            children: self.children.clone(),
        }
    }
}

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

impl<C> flui_view::View for Row<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl<C> flui_view::StatelessView for Row<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        let text_direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        FlexRenderView {
            direction: FlexDirection::Horizontal,
            style: self.style,
            text_direction,
            children: self.children.clone(),
        }
    }
}

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

impl<C> flui_view::View for Column<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl<C> flui_view::StatelessView for Column<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        let text_direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        FlexRenderView {
            direction: FlexDirection::Vertical,
            style: self.style,
            text_direction,
            children: self.children.clone(),
        }
    }
}

/// The actual `RenderFlex`-backed render-object widget shared by [`Flex`],
/// [`Row`], and [`Column`]. Its [`TextDirection`] is already resolved by the
/// composing widget's `StatelessView::build` — kept private so
/// `Directionality` is only ever read at the `BuildContext` seam that can see
/// it, never assumed to be reachable from a bare `RenderView`.
#[derive(Clone)]
struct FlexRenderView<C> {
    direction: FlexDirection,
    style: FlexStyle,
    text_direction: TextDirection,
    children: C,
}

impl<C> flui_view::RenderView for FlexRenderView<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.style.build(self.direction, self.text_direction)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.update_directions(
            self.direction,
            self.text_direction,
            self.style.text_baseline,
        ) | render_object.update_layout_configuration(
            self.style.main_axis_alignment,
            self.style.main_axis_size,
            self.style.cross_axis_alignment,
            self.style.spacing,
        )
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(FlexRenderView);

#[cfg(test)]
mod tests {
    //! `RenderFlex` exposes no public `spacing` getter, so wiring is verified
    //! through its derived `Debug` output — the same technique
    //! `crate::wrap::Wrap`'s own builder-wiring tests use for `RenderWrap`.
    //!
    //! `Row`/`Column` are composing `StatelessView`s (their `build` resolves
    //! `Directionality` before delegating to `FlexRenderView`), so these tests
    //! construct `FlexRenderView` directly with `TextDirection::Ltr` — the
    //! same value `build` falls back to with no ambient `Directionality` —
    //! rather than going through a `BuildContext`. `Directionality`
    //! resolution itself is covered by the RTL parity tests in
    //! `crates/flui-widgets/tests/parity/row_test.rs` and
    //! `crates/flui-widgets/tests/parity/column_test.rs`.

    use flui_view::RenderView;

    use super::*;

    fn as_render_view<C: ViewSeq + Clone + 'static>(
        direction: FlexDirection,
        style: FlexStyle,
        children: C,
    ) -> FlexRenderView<C> {
        FlexRenderView {
            direction,
            style,
            text_direction: TextDirection::Ltr,
            children,
        }
    }

    #[test]
    fn row_spacing_defaults_to_zero() {
        let row: Row = Row::new(Vec::new());
        let render_view = as_render_view(FlexDirection::Horizontal, row.style, row.children);
        let render_object =
            render_view.create_render_object(&flui_view::RenderObjectContext::detached());
        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("spacing: 0.0"),
            "Row's default spacing must be 0.0, got: {debug}"
        );
    }

    #[test]
    fn row_spacing_builder_reaches_render_flex() {
        let row: Row = Row::new(Vec::new()).spacing(8.0);
        let render_view = as_render_view(FlexDirection::Horizontal, row.style, row.children);
        let render_object =
            render_view.create_render_object(&flui_view::RenderObjectContext::detached());
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
        let initial_view =
            as_render_view(FlexDirection::Horizontal, initial.style, initial.children);
        let mut render_object =
            initial_view.create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(format!("{render_object:?}").contains("spacing: 1.0"));
        assert_eq!(
            initial_view.update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::NONE,
        );

        let updated: Row = Row::new(Vec::new()).spacing(9.0);
        let updated_view =
            as_render_view(FlexDirection::Horizontal, updated.style, updated.children);
        let impact = updated_view.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::LAYOUT);

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
        let render_view = as_render_view(FlexDirection::Vertical, column.style, column.children);
        let render_object =
            render_view.create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(
            format!("{render_object:?}").contains("spacing: 6.0"),
            "Column::spacing(6.0) must reach RenderFlex via the shared macro"
        );
    }
}
