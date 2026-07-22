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
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderWrap;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderWrap {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderWrap,
    ) {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats

    use flui_view::RenderView;
    use flui_view::ViewExt;

    use super::*;
    use crate::SizedBox;

    // ── Builder defaults ──────────────────────────────────────────────────────

    #[test]
    fn wrap_new_defaults_to_horizontal_start_zero_spacing() {
        let wrap: Wrap = Wrap::new(Vec::new());
        assert_eq!(wrap.direction, Axis::Horizontal);
        assert_eq!(wrap.alignment, WrapAlignment::Start);
        assert_eq!(wrap.spacing, 0.0);
        assert_eq!(wrap.run_alignment, WrapAlignment::Start);
        assert_eq!(wrap.run_spacing, 0.0);
        assert_eq!(wrap.cross_axis_alignment, WrapCrossAlignment::Start);
    }

    #[test]
    fn wrap_builders_override_every_field() {
        let wrap: Wrap = Wrap::new(Vec::new())
            .direction(Axis::Vertical)
            .alignment(WrapAlignment::SpaceEvenly)
            .spacing(6.0)
            .run_alignment(WrapAlignment::SpaceAround)
            .run_spacing(3.0)
            .cross_axis_alignment(WrapCrossAlignment::End);

        assert_eq!(wrap.direction, Axis::Vertical);
        assert_eq!(wrap.alignment, WrapAlignment::SpaceEvenly);
        assert_eq!(wrap.spacing, 6.0);
        assert_eq!(wrap.run_alignment, WrapAlignment::SpaceAround);
        assert_eq!(wrap.run_spacing, 3.0);
        assert_eq!(wrap.cross_axis_alignment, WrapCrossAlignment::End);
    }

    // ── create_render_object / update_render_object wiring ───────────────────
    //
    // RenderWrap exposes no public getters, so wiring is verified through its
    // derived `Debug` output (per its `#[derive(Debug, Clone)]`).

    #[test]
    fn wrap_create_render_object_wires_defaults_into_render_wrap() {
        let wrap: Wrap = Wrap::new(Vec::new());
        let render_object = wrap.create_render_object(&flui_view::RenderObjectContext::detached());
        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("direction: Horizontal")
                && debug.contains("alignment: Start")
                && debug.contains("spacing: 0.0")
                && debug.contains("run_alignment: Start")
                && debug.contains("run_spacing: 0.0")
                && debug.contains("cross_axis_alignment: Start"),
            "default RenderWrap must carry Wrap's default fields, got: {debug}",
        );
    }

    #[test]
    fn wrap_create_render_object_wires_overridden_fields_into_render_wrap() {
        let wrap: Wrap = Wrap::new(Vec::new())
            .direction(Axis::Vertical)
            .alignment(WrapAlignment::Center)
            .spacing(8.0)
            .run_alignment(WrapAlignment::End)
            .run_spacing(4.0)
            .cross_axis_alignment(WrapCrossAlignment::Center);
        let render_object = wrap.create_render_object(&flui_view::RenderObjectContext::detached());
        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("direction: Vertical")
                && debug.contains("alignment: Center")
                && debug.contains("spacing: 8.0")
                && debug.contains("run_alignment: End")
                && debug.contains("run_spacing: 4.0")
                && debug.contains("cross_axis_alignment: Center"),
            "overridden RenderWrap must carry Wrap's overridden fields, got: {debug}",
        );
    }

    #[test]
    fn wrap_update_render_object_replaces_stale_configuration() {
        let initial: Wrap = Wrap::new(Vec::new()).spacing(1.0);
        let mut render_object =
            initial.create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(format!("{render_object:?}").contains("spacing: 1.0"));

        let updated: Wrap = Wrap::new(Vec::new())
            .direction(Axis::Vertical)
            .alignment(WrapAlignment::SpaceBetween)
            .spacing(9.0)
            .run_alignment(WrapAlignment::Center)
            .run_spacing(5.0)
            .cross_axis_alignment(WrapCrossAlignment::End);
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("direction: Vertical")
                && debug.contains("alignment: SpaceBetween")
                && debug.contains("spacing: 9.0")
                && debug.contains("run_alignment: Center")
                && debug.contains("run_spacing: 5.0")
                && debug.contains("cross_axis_alignment: End"),
            "update_render_object must overwrite the stale config, got: {debug}",
        );
        assert!(
            !debug.contains("spacing: 1.0"),
            "update_render_object must not retain the pre-update spacing, got: {debug}",
        );
    }

    // ── has_children ───────────────────────────────────────────────────────────

    #[test]
    fn wrap_has_children_is_false_for_an_empty_child_list() {
        let wrap: Wrap = Wrap::new(Vec::new());
        assert!(!wrap.has_children());
    }

    #[test]
    fn wrap_has_children_is_true_when_children_are_present() {
        let wrap = Wrap::new(vec![SizedBox::shrink().boxed()]);
        assert!(wrap.has_children());
    }

    // ── Debug ──────────────────────────────────────────────────────────────────

    #[test]
    fn wrap_debug_reports_all_fields_and_child_count() {
        let wrap = Wrap::new(vec![SizedBox::shrink().boxed(), SizedBox::shrink().boxed()])
            .direction(Axis::Vertical)
            .alignment(WrapAlignment::Center)
            .spacing(2.0)
            .run_alignment(WrapAlignment::End)
            .run_spacing(1.0)
            .cross_axis_alignment(WrapCrossAlignment::Center);

        let debug = format!("{wrap:?}");
        assert!(
            debug.contains("direction: Vertical")
                && debug.contains("alignment: Center")
                && debug.contains("spacing: 2.0")
                && debug.contains("run_alignment: End")
                && debug.contains("run_spacing: 1.0")
                && debug.contains("cross_axis_alignment: Center")
                && debug.contains("children: 2"),
            "Wrap Debug must include every field and the child count, got: {debug}",
        );
    }
}
