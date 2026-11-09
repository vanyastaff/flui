//! [`Flex`], [`Row`], and [`Column`] — lay out children along an axis.

use std::fmt;

use flui_objects::{
    CrossAxisAlignment, FlexDirection, MainAxisAlignment, MainAxisSize, RenderFlex,
    VerticalDirection,
};
use flui_painting::typography::TextBaseline;
use flui_painting::typography::TextDirection;
use flui_rendering::protocol::BoxProtocol;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, CreationSite, InheritedView};

use crate::localization::Directionality;
use crate::support::generic_render_view_element;

/// Shared main/cross-axis configuration for the flex family, with defaults
/// (`MainAxisAlignment::Start`, `CrossAxisAlignment::Center`,
/// `MainAxisSize::Max`, `spacing: 0.0`).
///
/// `text_baseline` is a plain [`TextBaseline`] rather than an optional value
/// required only under `CrossAxisAlignment::Baseline`, so it defaults to
/// `Alphabetic` — the value that makes the baseline case work and that every
/// other alignment ignores.
#[derive(Clone, Copy, Debug)]
struct FlexStyle {
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    main_axis_size: MainAxisSize,
    spacing: f64,
    text_baseline: TextBaseline,
    vertical_direction: VerticalDirection,
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
            spacing: 0.0,
            text_baseline: TextBaseline::Alphabetic,
            vertical_direction: VerticalDirection::Down,
        }
    }
}

impl FlexStyle {
    /// `text_direction` is the already-resolved ambient direction, resolved by
    /// [`resolve_text_direction`] at the render-object seam rather than looked
    /// up here.
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
            .with_vertical_direction(self.vertical_direction)
    }
}

/// Generate the shared builder methods (main/cross alignment, main-axis size)
/// for a flex-family widget that stores its config in a `style: FlexStyle`.
macro_rules! flex_style_builders {
    () => {
        /// Which vertical edge is the start, for either layout axis.
        #[must_use]
        pub fn vertical_direction(mut self, direction: VerticalDirection) -> Self {
            self.style.vertical_direction = direction;
            self
        }

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
        /// Applied
        /// strictly *between* children (never before the first or after the
        /// last), regardless of [`MainAxisAlignment`]. Defaults to `0.0`.
        #[must_use]
        pub fn spacing(mut self, spacing: f64) -> Self {
            self.style.spacing = spacing;
            self
        }
    };
}

/// Lays out children along a configurable [`FlexDirection`].
///
/// Prefer
/// [`Row`] / [`Column`] for the common fixed-direction cases.
///
/// Generic over `C: ViewSeq`: a static `column!`/`row!` tuple keeps each child
/// monomorphic (the contract-C2 fast path), while a `Vec<BoxedView>` carries a
/// dynamic, runtime-sized child list.
///
/// Resolves the ambient [`Directionality`] as follows: a horizontal flex (`Row`) consults it for
/// its *main* axis (`Start`/`End` and child order flip under `Rtl`); a
/// vertical flex (`Column`) consults it for its *cross* axis instead — its
/// main axis is governed by [`Self::vertical_direction`].
/// Defaults to [`TextDirection::Ltr`] with no ancestor, matching every other
/// FLUI widget that consults `Directionality`. The read goes through
/// [`RenderObjectContext::depend_on`](flui_view::RenderObjectContext::depend_on),
/// so the render object re-pushes its configuration when the direction changes.
#[derive(Clone)]
pub struct Flex<C = Vec<BoxedView>> {
    direction: FlexDirection,
    style: FlexStyle,
    children: C,
    site: CreationSite,
}

impl<C> Flex<C> {
    /// A flex laid out along `direction` with the given children.
    #[track_caller]
    pub fn new(direction: FlexDirection, children: C) -> Self {
        Self {
            direction,
            style: FlexStyle::default(),
            children,
            site: CreationSite::here(),
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
            .finish_non_exhaustive()
    }
}

impl<C> flui_view::RenderView for Flex<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.style.build(
            self.direction,
            resolve_text_direction(self.direction, self.style.cross_axis_alignment, ctx),
        )
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        update_flex(
            render_object,
            self.direction,
            self.style,
            resolve_text_direction(self.direction, self.style.cross_axis_alignment, ctx),
        )
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Flex, site = site);

/// Lays out children horizontally.
#[derive(Clone)]
pub struct Row<C = Vec<BoxedView>> {
    style: FlexStyle,
    children: C,
    site: CreationSite,
}

impl<C> Row<C> {
    /// A horizontal row of the given children.
    #[track_caller]
    pub fn new(children: C) -> Self {
        Self {
            style: FlexStyle::default(),
            children,
            site: CreationSite::here(),
        }
    }

    flex_style_builders!();
}

impl<C: ViewSeq> fmt::Debug for Row<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Row")
            .field("style", &self.style)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<C> flui_view::RenderView for Row<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.style.build(
            FlexDirection::Horizontal,
            resolve_text_direction(
                FlexDirection::Horizontal,
                self.style.cross_axis_alignment,
                ctx,
            ),
        )
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        update_flex(
            render_object,
            FlexDirection::Horizontal,
            self.style,
            resolve_text_direction(
                FlexDirection::Horizontal,
                self.style.cross_axis_alignment,
                ctx,
            ),
        )
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Row, site = site);

/// Lays out children vertically.
#[derive(Clone)]
pub struct Column<C = Vec<BoxedView>> {
    style: FlexStyle,
    children: C,
    site: CreationSite,
}

impl<C> Column<C> {
    /// A vertical column of the given children.
    #[track_caller]
    pub fn new(children: C) -> Self {
        Self {
            style: FlexStyle::default(),
            children,
            site: CreationSite::here(),
        }
    }

    flex_style_builders!();
}

impl<C: ViewSeq> fmt::Debug for Column<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Column")
            .field("style", &self.style)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<C> flui_view::RenderView for Column<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.style.build(
            FlexDirection::Vertical,
            resolve_text_direction(
                FlexDirection::Vertical,
                self.style.cross_axis_alignment,
                ctx,
            ),
        )
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        update_flex(
            render_object,
            FlexDirection::Vertical,
            self.style,
            resolve_text_direction(
                FlexDirection::Vertical,
                self.style.cross_axis_alignment,
                ctx,
            ),
        )
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Column, site = site);

/// Whether this flex needs the ambient [`Directionality`] at all.
///
/// A HORIZONTAL flex always needs it, "because it affects the layout order"; a VERTICAL one
/// needs it only when its cross-axis alignment is direction-sensitive
/// (`Start`/`End`), because those name a reading edge rather than a physical
/// one.
///
/// This gates the LOOKUP, not just its result. `depend_on` registers an
/// inherited dependency, so calling it unconditionally makes every `Column`
/// rebuild whenever the ambient direction changes -- even a centred one whose
/// layout cannot move. Skipping the call avoids that.
fn needs_text_direction(direction: FlexDirection, cross: CrossAxisAlignment) -> bool {
    match direction {
        FlexDirection::Horizontal => true,
        FlexDirection::Vertical => {
            matches!(cross, CrossAxisAlignment::Start | CrossAxisAlignment::End)
        }
    }
}

/// Resolve the flex's [`TextDirection`] from the ambient [`Directionality`],
/// gated by [`needs_text_direction`] so a centred column registers no dependency.
/// At mount the inherited read is not yet available, so it defaults to `Ltr` and
/// the first rebuild re-resolves it (T09).
fn resolve_text_direction(
    direction: FlexDirection,
    cross: CrossAxisAlignment,
    ctx: &flui_view::RenderObjectContext<'_>,
) -> TextDirection {
    if needs_text_direction(direction, cross) {
        ctx.depend_on::<Directionality, _>(|d| *d.data())
            .unwrap_or(TextDirection::Ltr)
    } else {
        TextDirection::Ltr
    }
}

/// Push a flex's resolved configuration onto `render_object`, shared by
/// [`Flex`], [`Row`] and [`Column`].
fn update_flex(
    render_object: &mut RenderFlex,
    direction: FlexDirection,
    style: FlexStyle,
    text_direction: TextDirection,
) -> flui_rendering::RenderUpdateImpact {
    render_object.update_directions(direction, text_direction, style.text_baseline)
        | render_object.update_layout_configuration(
            style.main_axis_alignment,
            style.main_axis_size,
            style.cross_axis_alignment,
            style.spacing,
        )
        | render_object.set_vertical_direction(style.vertical_direction)
}
