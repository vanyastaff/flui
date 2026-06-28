//! [`Wrap`] widget — Flutter-parity flow layout over [`RenderWrap`].

use std::fmt;

use flui_objects::{RenderWrap, WrapAlignment, WrapCrossAlignment};
use flui_rendering::protocol::BoxProtocol;
use flui_types::layout::Axis;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Lays children out along [`direction`](Wrap::direction), wrapping to a new
/// run when the next child would overflow the available main-axis extent.
///
/// Flutter parity: `widgets/basic.dart` `Wrap` over [`RenderWrap`].
///
/// # Direction and axis terminology
///
/// The *main axis* is the axis along which children are placed before wrapping.
/// The *cross axis* is perpendicular — each wrap creates a new *run* in this
/// direction. For the default horizontal direction the main axis is horizontal
/// (left-to-right in LTR) and the cross axis is vertical (top-to-bottom).
///
/// # RTL caveat
///
/// `WrapAlignment::Start` / `End` and `WrapCrossAlignment::Start` / `End` are
/// always LTR / TTB in FLUI — `TextDirection` is not yet plumbed into layout.
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
    spacing: f32,
    run_alignment: WrapAlignment,
    run_spacing: f32,
    cross_axis_alignment: WrapCrossAlignment,
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
            children,
        }
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
    pub fn spacing(mut self, spacing: f32) -> Self {
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
    pub fn run_spacing(mut self, run_spacing: f32) -> Self {
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
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for Wrap<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderWrap;

    fn create_render_object(&self) -> RenderWrap {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut RenderWrap) {
        *render_object = self.build_render_object();
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Wrap);
