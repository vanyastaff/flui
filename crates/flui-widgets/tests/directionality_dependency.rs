//! Criterion 4 of #538: a vertical layout must not take a `Directionality`
//! dependency it cannot use.
//!
//! `Directionality::maybe_of` REGISTERS an inherited dependency, so calling it
//! unconditionally makes every `Column` rebuild when the ambient direction
//! changes -- including a centred one whose layout cannot move. The reference
//! gates the lookup itself (`widgets/basic.dart`'s `Flex._needTextDirection`:
//! horizontal always; vertical only for `CrossAxisAlignment::Start`/`End`,
//! because those name a reading edge rather than a physical one).
//!
//! The oracle asks the dependency directly. A rebuild-counting version of this
//! test was written first and is NOT usable: changing an inherited value means
//! rebuilding the widget that provides it, which rebuilds the whole subtree
//! regardless of who depends on what, so it reports the same count for a
//! dependent and a non-dependent child. It looked convincing -- red exactly
//! like the defect -- and applying the fix did not turn it green.

use crate::common::{lay_out, tight};
use flui_painting::typography::TextDirection;
use flui_painting::{Alignment, AlignmentDirectional};
use flui_rendering::constraints::AxisDirection;
use flui_widgets::{Align, Column, CrossAxisAlignment, Directionality, SizedBox, Stack, Viewport};

/// Dependents of the ambient `Directionality` for a `Column` with `cross`.
fn directionality_dependents(cross: CrossAxisAlignment) -> usize {
    let mut laid = lay_out(
        Directionality::new(
            TextDirection::Ltr,
            Column::new((SizedBox::square(10.0),)).cross_axis_alignment(cross),
        ),
        tight(200.0, 200.0),
    );
    laid.inherited_dependent_count::<Directionality>()
}

/// `Start` names a READING edge, so a direction change genuinely moves this
/// column's children. It must take the dependency.
///
/// This is the control that keeps its sibling honest: without it, a zero there
/// could mean the lookup is gated correctly OR that nothing ever registers.
pub(crate) fn a_start_aligned_column_depends_on_directionality() {
    assert_eq!(
        directionality_dependents(CrossAxisAlignment::Start),
        1,
        "CrossAxisAlignment::Start resolves against the reading direction, so \
         the column must depend on Directionality"
    );
}

/// Dependents of the ambient `Directionality` for a render-object widget mounted
/// under it.
fn render_dependents(view: impl flui_view::View) -> usize {
    let mut laid = lay_out(
        Directionality::new(TextDirection::Ltr, view),
        tight(200.0, 200.0),
    );
    laid.inherited_dependent_count::<Directionality>()
}

/// An absolute alignment names physical edges, so `Align` must not register a
/// `Directionality` dependency; a directional one names a reading edge and must.
/// `Stack` always registers one — it resolves the reading direction for its
/// directional `Positioned` children's `start`/`end` edges, which it cannot see
/// statically.
pub(crate) fn directional_geometry_registers_a_direction_dependency_only_when_directional() {
    let absolute_align =
        render_dependents(Align::new(Alignment::CENTER).child(SizedBox::square(10.0)));
    assert_eq!(
        absolute_align, 0,
        "an absolute alignment names physical edges: no Directionality dependency"
    );
    let directional_align = render_dependents(
        Align::new(AlignmentDirectional::TOP_START).child(SizedBox::square(10.0)),
    );
    assert_eq!(
        directional_align, 1,
        "a directional alignment names a reading edge"
    );

    let absolute_stack = render_dependents(Stack::new((SizedBox::square(10.0),)));
    assert_eq!(
        absolute_stack, 1,
        "Stack always resolves the reading direction for Positioned start/end"
    );
    let directional_stack = render_dependents(
        Stack::new((SizedBox::square(10.0),)).alignment(AlignmentDirectional::TOP_START),
    );
    assert_eq!(
        directional_stack, 1,
        "a directional Stack alignment names a reading edge"
    );
}

/// A vertical viewport lays its horizontal cross axis against the reading
/// direction, so it must depend on it; a horizontal one lays a fixed
/// top-to-bottom cross axis and must not.
pub(crate) fn a_vertical_viewport_depends_on_directionality_but_a_horizontal_one_does_not() {
    let vertical = render_dependents(Viewport::new((SizedBox::square(10.0),)));
    assert_eq!(
        vertical, 1,
        "a vertical viewport's cross axis is horizontal (reading)"
    );
    let horizontal = render_dependents(
        Viewport::new((SizedBox::square(10.0),)).axis_direction(AxisDirection::LeftToRight),
    );
    assert_eq!(
        horizontal, 0,
        "a horizontal viewport's cross axis is top-to-bottom"
    );
}

/// What a [`DirectionProbe`]'s `update_render_object` read, one entry per call.
type DirectionLog = std::rc::Rc<std::cell::RefCell<Vec<Option<TextDirection>>>>;

/// A leaf `RenderView` whose `update_render_object` reads the ambient
/// `Directionality` through `RenderObjectContext::depend_on`, logs what it
/// read, and panics on `Rtl` while `fail` is set.
#[derive(Clone)]
struct DirectionProbe {
    log: DirectionLog,
    fail: std::rc::Rc<std::cell::Cell<bool>>,
}

impl flui_view::RenderView for DirectionProbe {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        flui_objects::RenderSizedBox::new(Some(10.0), Some(10.0))
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let direction = ctx.depend_on::<Directionality, _>(|d| *flui_view::InheritedView::data(d));
        self.log.borrow_mut().push(direction);
        assert!(
            !(self.fail.get() && direction == Some(TextDirection::Rtl)),
            "DirectionProbe refuses Rtl"
        );
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl flui_view::View for DirectionProbe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

fn probe() -> (
    DirectionProbe,
    DirectionLog,
    std::rc::Rc<std::cell::Cell<bool>>,
) {
    let log = DirectionLog::default();
    let fail = std::rc::Rc::new(std::cell::Cell::new(false));
    let probe = DirectionProbe {
        log: std::rc::Rc::clone(&log),
        fail: std::rc::Rc::clone(&fail),
    };
    (probe, log, fail)
}

/// A parent that hands a render view a new configuration reconfigures it once,
/// with the inherited data resolved: the update does not first push a
/// configuration read without the tree (`None`, which a widget turns into
/// `Ltr`) and then the real one.
pub(crate) fn a_swapped_render_view_reconfigures_once_with_its_inherited_data() {
    let (view, log, _fail) = probe();
    let mut laid = lay_out(
        Directionality::new(TextDirection::Rtl, view.clone()),
        tight(200.0, 200.0),
    );
    assert_eq!(
        *log.borrow(),
        [Some(TextDirection::Rtl)],
        "the first build reconfigures the mounted render object with the ambient direction"
    );

    log.borrow_mut().clear();
    laid.pump_widget(Directionality::new(TextDirection::Rtl, view));
    assert_eq!(
        *log.borrow(),
        [Some(TextDirection::Rtl)],
        "a view swap runs update_render_object once, with the tree in reach"
    );
}

/// A render view whose `update_render_object` panics on a dependency rebuild is
/// replaced by the error view and the next frame
/// runs: the failing element is not requeued.
pub(crate) fn a_render_update_panic_on_a_dependency_rebuild_shows_the_error_view() {
    use crate::media_query_fields::StaticChild;
    use flui_view::ViewExt;

    let (view, log, fail) = probe();
    let preserved = |view: &DirectionProbe| StaticChild {
        inner: view.clone().boxed(),
    };
    let mut laid = lay_out(
        Directionality::new(TextDirection::Ltr, preserved(&view)),
        tight(200.0, 200.0),
    );
    assert!(laid.try_find_by_render_type("RenderErrorBox").is_none());

    fail.set(true);
    laid.pump_widget(Directionality::new(TextDirection::Rtl, preserved(&view)));
    assert_eq!(
        log.borrow().last(),
        Some(&Some(TextDirection::Rtl)),
        "the direction change reached the preserved probe as a dependency rebuild"
    );
    assert!(
        laid.try_find_by_render_type("RenderErrorBox").is_some(),
        "the panicking render view is replaced by the error view"
    );

    fail.set(false);
    laid.pump();
    laid.pump_widget(Directionality::new(TextDirection::Rtl, view));
    assert!(
        laid.try_find_by_render_type("RenderErrorBox").is_none(),
        "the next frame mounts the fixed view in the error view's place"
    );
    assert_eq!(log.borrow().last(), Some(&Some(TextDirection::Rtl)));
}

/// Flipping the ambient direction above a preserved `Row` and `Align` reaches
/// their render objects as a dependency rebuild: the same render objects (no
/// remount) lay their children out from the other edge.
pub(crate) fn a_direction_flip_reconfigures_preserved_render_objects_in_place() {
    use crate::media_query_fields::StaticChild;
    use flui_view::ViewExt;
    use flui_widgets::Row;

    let row = || StaticChild {
        inner: Row::new((SizedBox::new(50.0, 50.0), SizedBox::new(30.0, 50.0))).boxed(),
    };
    let align = || StaticChild {
        inner: Align::new(AlignmentDirectional::CENTER_START)
            .child(SizedBox::new(40.0, 40.0))
            .boxed(),
    };

    for (name, view, ltr_x, rtl_x, render_type) in [
        ("Row", row().boxed(), 0.0, 150.0, "RenderFlex"),
        ("Align", align().boxed(), 0.0, 160.0, "RenderAlign"),
    ] {
        let mut laid = lay_out(
            Directionality::new(TextDirection::Ltr, view.clone()),
            tight(200.0, 200.0),
        );
        let object = laid.find_by_render_type(render_type);
        let first_child_x = |laid: &crate::common::LaidOut| {
            let object = laid.find_by_render_type(render_type);
            laid.offset(laid.children(object)[0]).dx
        };
        assert_eq!(
            first_child_x(&laid),
            ltr_x,
            "{name}: Ltr starts at the left"
        );

        laid.pump_widget(Directionality::new(TextDirection::Rtl, view.clone()));
        assert_eq!(
            laid.find_by_render_type(render_type),
            object,
            "{name}: the render object is preserved, not remounted"
        );
        assert_eq!(
            first_child_x(&laid),
            rtl_x,
            "{name}: Rtl starts at the right"
        );

        laid.pump_widget(Directionality::new(TextDirection::Ltr, view));
        assert_eq!(laid.find_by_render_type(render_type), object);
        assert_eq!(first_child_x(&laid), ltr_x, "{name}: back to Ltr");
    }
}
