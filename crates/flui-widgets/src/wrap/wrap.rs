//! [`Wrap`] widget — flow layout over [`RenderWrap`].

use std::fmt;

use flui_foundation::geometry::Axis;
use flui_objects::{RenderWrap, WrapAlignment, WrapCrossAlignment};
use flui_painting::paint::Clip;
use flui_painting::typography::TextDirection;
use flui_rendering::protocol::BoxProtocol;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::Directionality;
use crate::support::generic_render_view_element;

/// Lays children out along [`direction`](Wrap::direction), wrapping to a new
/// run when the next child would overflow the available main-axis extent.
///
/// # Direction and axis terminology
///
/// The *main axis* is the axis along which children are placed before wrapping.
/// The *cross axis* is perpendicular — each wrap creates a new *run* in this
/// direction. For the default horizontal direction the main axis is horizontal
/// (left-to-right in LTR) and the cross axis is vertical (top-to-bottom).
///
/// # Reading direction
///
/// The ambient [`Directionality`] decides where a horizontal run starts and
/// which side a vertical wrap places its first run on: in a right-to-left
/// subtree both start at the right, and `Start` / `End` alignments name the
/// reading edge. Horizontal runs stack top to bottom, and a vertical run
/// fills from the top.
///
/// # Example
///
/// ```rust,ignore
/// Wrap::new(vec![
///     SizedBox::new(80.0, 40.0).boxed(),
///     SizedBox::new(80.0, 40.0).boxed(),
///     SizedBox::new(80.0, 40.0).boxed(),
/// ])
/// .spacing(8.0)
/// .run_spacing(4.0)
/// .alignment(WrapAlignment::Center)
/// ```
///
/// Generic over `C: ViewSeq`: a `Vec<BoxedView>` carries a dynamic child list;
/// a static tuple via `column!` / `row!` keeps each child monomorphic.
#[derive(Clone)]
pub struct Wrap<C = Vec<BoxedView>> {
    direction: Axis,
    alignment: WrapAlignment,
    spacing: f64,
    run_alignment: WrapAlignment,
    run_spacing: f64,
    cross_axis_alignment: WrapCrossAlignment,
    clip_behavior: Clip,
    children: C,
}

impl<C> Wrap<C> {
    /// Creates a horizontal `Wrap` with all alignments defaulting to
    /// [`WrapAlignment::Start`] and zero spacing.
    pub fn new(children: C) -> Self {
        Self {
            direction: Axis::Horizontal,
            alignment: WrapAlignment::Start,
            spacing: 0.0,
            run_alignment: WrapAlignment::Start,
            run_spacing: 0.0,
            cross_axis_alignment: WrapCrossAlignment::Start,
            clip_behavior: Clip::None,
            children,
        }
    }

    /// How content that overflows this wrap is clipped when it paints.
    ///
    /// `Clip::None` by default, matching the reference: a wrap is a layout
    /// rather than a viewport, and most never overflow, so it does not pay
    /// for a clip layer it does not need. A wrap that DOES overflow paints
    /// past its own bounds until this is set.
    #[must_use]
    pub const fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Sets the main-axis direction (`Horizontal` by default).
    #[must_use]
    pub fn direction(mut self, direction: Axis) -> Self {
        self.direction = direction;
        self
    }

    /// Sets how children are aligned within each run along the main axis.
    #[must_use]
    pub fn alignment(mut self, alignment: WrapAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets the minimum gap between adjacent children within a run.
    #[must_use]
    pub fn spacing(mut self, spacing: f64) -> Self {
        self.spacing = spacing;
        self
    }

    /// Sets how runs are aligned along the cross axis.
    #[must_use]
    pub fn run_alignment(mut self, run_alignment: WrapAlignment) -> Self {
        self.run_alignment = run_alignment;
        self
    }

    /// Sets the minimum gap between adjacent runs.
    #[must_use]
    pub fn run_spacing(mut self, run_spacing: f64) -> Self {
        self.run_spacing = run_spacing;
        self
    }

    /// Sets how each child is positioned within its run on the cross axis.
    #[must_use]
    pub fn cross_axis_alignment(mut self, alignment: WrapCrossAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    fn build_render_object(&self) -> RenderWrap {
        RenderWrap::new()
            .with_direction(self.direction)
            .with_alignment(self.alignment)
            .with_spacing(self.spacing)
            .with_run_alignment(self.run_alignment)
            .with_run_spacing(self.run_spacing)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_clip_behavior(self.clip_behavior)
    }
}

impl<C: ViewSeq> fmt::Debug for Wrap<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wrap")
            .field("direction", &self.direction)
            .field("alignment", &self.alignment)
            .field("spacing", &self.spacing)
            .field("run_alignment", &self.run_alignment)
            .field("run_spacing", &self.run_spacing)
            .field("cross_axis_alignment", &self.cross_axis_alignment)
            .field("clip_behavior", &self.clip_behavior)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::View for Wrap<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl<C> flui_view::StatelessView for Wrap<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        WrapRenderView {
            wrap: self.clone(),
            text_direction: Directionality::maybe_of(ctx).unwrap_or_default(),
        }
    }
}

/// The `RenderWrap`-backed render-object widget behind [`Wrap`], with the
/// ambient direction already read at the `BuildContext` seam: a horizontal
/// run starts at the reading edge, and a vertical wrap places its runs from
/// it.
#[derive(Clone)]
struct WrapRenderView<C> {
    wrap: Wrap<C>,
    text_direction: TextDirection,
}

impl<C> flui_view::RenderView for WrapRenderView<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderWrap;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderWrap {
        self.wrap
            .build_render_object()
            .with_text_direction(self.text_direction)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderWrap,
    ) -> flui_rendering::RenderUpdateImpact {
        let wrap = &self.wrap;
        render_object.update_configuration(
            wrap.direction,
            wrap.alignment,
            wrap.spacing,
            wrap.run_alignment,
            wrap.run_spacing,
            wrap.cross_axis_alignment,
        ) | render_object.set_text_direction(self.text_direction)
            | render_object.set_clip_behavior(wrap.clip_behavior)
    }

    fn has_children(&self) -> bool {
        !self.wrap.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.wrap.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(WrapRenderView);
