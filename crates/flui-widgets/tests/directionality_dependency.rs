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
use flui_foundation::TextDirection;
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
    let vertical = render_dependents(Viewport::new(
        flui_rendering::view::ScrollPosition::fixed(0.0),
        (SizedBox::square(10.0),),
    ));
    assert_eq!(
        vertical, 1,
        "a vertical viewport's cross axis is horizontal (reading)"
    );
    let horizontal = render_dependents(
        Viewport::new(
            flui_rendering::view::ScrollPosition::fixed(0.0),
            (SizedBox::square(10.0),),
        )
        .axis_direction(AxisDirection::LeftToRight),
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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
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

/// Flipping the ambient direction above a preserved `Row`, `Align` and `Table` reaches
/// their render objects as a dependency rebuild: the same render objects (no
/// remount) lay their children out from the other edge.
pub(crate) fn a_direction_flip_reconfigures_preserved_render_objects_in_place() {
    use crate::media_query_fields::StaticChild;
    use flui_view::ViewExt;
    use flui_widgets::{Row, Table, TableRow};

    let row = || StaticChild {
        inner: Row::new((SizedBox::new(50.0, 50.0), SizedBox::new(30.0, 50.0))).boxed(),
    };
    let align = || StaticChild {
        inner: Align::new(AlignmentDirectional::CENTER_START)
            .child(SizedBox::new(40.0, 40.0))
            .boxed(),
    };
    // Two columns of the default `Flex(1.0)`, so each is 100 wide.
    let table = || StaticChild {
        inner: Table::new(vec![TableRow::new(vec![
            SizedBox::new(50.0, 50.0).boxed(),
            SizedBox::new(30.0, 50.0).boxed(),
        ])])
        .boxed(),
    };

    for (name, view, ltr_x, rtl_x, render_type) in [
        ("Row", row().boxed(), 0.0, 150.0, "RenderFlex"),
        ("Align", align().boxed(), 0.0, 160.0, "RenderAlign"),
        ("Table", table().boxed(), 0.0, 100.0, "RenderTable"),
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

/// Each shifted box takes an `AlignmentGeometry`: a directional alignment
/// resolves against the ambient direction, `TOP_START` naming the top-left
/// corner in LTR and the top-right one in RTL, and only a directional one
/// takes a `Directionality` dependency. Built with no alignment, each keeps
/// its default and takes none.
pub(crate) fn shifted_boxes_resolve_a_directional_alignment_against_the_ambient_direction() {
    use crate::common::{LaidOut, loose};
    use flui_foundation::geometry::{Matrix4, Size};
    use flui_objects::{RenderFittedBox, RenderTransform};
    use flui_painting::AlignmentGeometry;
    use flui_view::{BoxedView, ViewExt};
    use flui_widgets::{
        FittedBox, FractionallySizedBox, OverflowBox, SizedOverflowBox, Transform, UnconstrainedBox,
    };

    // The alignment a render object of type `T` was configured with.
    fn configured<T: flui_rendering::RenderObject<flui_rendering::protocol::BoxProtocol>>(
        laid: &LaidOut,
        render_type: &str,
        alignment: impl Fn(&T) -> Option<Alignment>,
    ) -> Option<(f64, f64)> {
        let id = laid.find_by_render_type(render_type);
        laid.pipeline_owner().with(|owner| {
            let render = owner
                .render_tree()
                .get(id)
                .and_then(|node| node.as_box())
                .and_then(|node| node.render_object().downcast_ref::<T>())
                .expect("the shifted box is mounted");
            alignment(render).map(|alignment| (alignment.x, alignment.y))
        })
    }
    // Where a box placed its child.
    fn child_offset(laid: &LaidOut, render_type: &str) -> (f64, f64) {
        let id = laid.find_by_render_type(render_type);
        let offset = laid.offset(laid.only_child(id));
        (offset.dx, offset.dy)
    }

    // Each row builds the box with `alignment`, or with none at all.
    type Make = fn(Option<AlignmentGeometry>) -> BoxedView;
    type Read = fn(&LaidOut) -> Option<(f64, f64)>;
    type Reading = Option<(f64, f64)>;
    // A row: its name, the constraints it is laid out under, how to build it,
    // how to read it, and the readings in LTR, in RTL and with no alignment.
    // A layout box is read where it placed its child, under a tight box so its
    // alignment has room to move the child.
    type Case = (
        &'static str,
        flui_rendering::constraints::BoxConstraints,
        Make,
        Read,
        (Reading, Reading, Reading),
    );
    let cases: [Case; 6] = [
        (
            "FittedBox",
            loose(200.0),
            |alignment| {
                let fitted = FittedBox::new().child(SizedBox::new(40.0, 40.0));
                alignment
                    .map_or(fitted.clone(), |alignment| fitted.alignment(alignment))
                    .boxed()
            },
            |laid| {
                configured(laid, "RenderFittedBox", |r: &RenderFittedBox| {
                    Some(r.alignment())
                })
            },
            (Some((-1.0, -1.0)), Some((1.0, -1.0)), Some((0.0, 0.0))),
        ),
        (
            "FractionallySizedBox",
            tight(200.0, 200.0),
            |alignment| {
                let sized = FractionallySizedBox::new()
                    .width_factor(0.5)
                    .child(SizedBox::new(40.0, 40.0));
                alignment
                    .map_or(sized.clone(), |alignment| sized.alignment(alignment))
                    .boxed()
            },
            |laid| Some(child_offset(laid, "RenderFractionallySizedBox")),
            // Under the tight 200×200 box the child is 100 wide and 200 tall.
            (Some((0.0, 0.0)), Some((100.0, 0.0)), Some((50.0, 0.0))),
        ),
        (
            "UnconstrainedBox",
            tight(200.0, 200.0),
            |alignment| {
                let unconstrained = UnconstrainedBox::new().child(SizedBox::new(40.0, 40.0));
                alignment
                    .map_or(unconstrained.clone(), |alignment| {
                        unconstrained.alignment(alignment)
                    })
                    .boxed()
            },
            |laid| Some(child_offset(laid, "RenderConstraintsTransformBox")),
            (Some((0.0, 0.0)), Some((160.0, 0.0)), Some((80.0, 80.0))),
        ),
        (
            "Transform",
            loose(200.0),
            |alignment| {
                let transform = Transform::new(Matrix4::IDENTITY).child(SizedBox::new(40.0, 40.0));
                alignment
                    .map_or(transform.clone(), |alignment| {
                        transform.alignment(alignment)
                    })
                    .boxed()
            },
            |laid| configured(laid, "RenderTransform", RenderTransform::alignment),
            // `Transform::new` has no pivot alignment until one is set.
            (Some((-1.0, -1.0)), Some((1.0, -1.0)), None),
        ),
        (
            "OverflowBox",
            loose(200.0),
            |alignment| {
                let overflow = OverflowBox::new()
                    .min_width(40.0)
                    .max_width(40.0)
                    .child(SizedBox::new(40.0, 40.0));
                alignment
                    .map_or(overflow.clone(), |alignment| overflow.alignment(alignment))
                    .boxed()
            },
            |laid| Some(child_offset(laid, "RenderConstrainedOverflowBox")),
            (Some((0.0, 0.0)), Some((160.0, 0.0)), Some((80.0, 80.0))),
        ),
        (
            "SizedOverflowBox",
            loose(200.0),
            |alignment| {
                let overflow =
                    SizedOverflowBox::new(Size::new(200.0, 200.0)).child(SizedBox::new(40.0, 40.0));
                alignment
                    .map_or(overflow.clone(), |alignment| overflow.alignment(alignment))
                    .boxed()
            },
            |laid| Some(child_offset(laid, "RenderSizedOverflowBox")),
            (Some((0.0, 0.0)), Some((160.0, 0.0)), Some((80.0, 80.0))),
        ),
    ];

    for (name, constraints, make, read, (ltr, rtl, default)) in cases {
        let directional = Some(AlignmentGeometry::from(AlignmentDirectional::TOP_START));
        let mut laid = lay_out(
            Directionality::new(TextDirection::Ltr, make(directional)),
            constraints,
        );
        assert_eq!(read(&laid), ltr, "{name}: start is the left edge in LTR");
        assert_eq!(
            laid.inherited_dependent_count::<Directionality>(),
            1,
            "{name}: a directional alignment depends on the direction"
        );
        laid.pump_widget(Directionality::new(TextDirection::Rtl, make(directional)));
        assert_eq!(read(&laid), rtl, "{name}: start is the right edge in RTL");

        for (alignment, expected, what) in [
            (
                Some(Alignment::TOP_LEFT.into()),
                ltr,
                "an absolute alignment",
            ),
            (None, default, "the default alignment"),
        ] {
            let mut laid = lay_out(
                Directionality::new(TextDirection::Rtl, make(alignment)),
                constraints,
            );
            assert_eq!(read(&laid), expected, "{name}: {what} stays put under RTL");
            assert_eq!(
                laid.inherited_dependent_count::<Directionality>(),
                0,
                "{name}: {what} takes no dependency"
            );
        }
    }
}

/// A direction flip that moves a `FittedBox`'s directional alignment moves the
/// child's published semantics with its pixels: the fitted transform places
/// both, so the flip re-places the child's node from the left edge to the
/// right one.
pub(crate) fn a_fitted_child_moved_by_a_direction_flip_moves_its_semantics() {
    use crate::media_query_fields::StaticChild;
    use flui_painting::BoxFit;
    use flui_view::ViewExt;
    use flui_widgets::{FittedBox, Semantics};

    let fitted = || {
        StaticChild {
            inner: FittedBox::new()
                .fit(BoxFit::None)
                .alignment(AlignmentDirectional::TOP_START)
                .child(
                    Semantics::new()
                        .label("fitted")
                        .child(SizedBox::new(40.0, 40.0)),
                )
                .boxed(),
        }
        .boxed()
    };
    let mut laid = lay_out(
        Directionality::new(TextDirection::Ltr, fitted()),
        tight(200.0, 200.0),
    );
    laid.enable_semantics();
    laid.tick();
    let left = |laid: &crate::common::LaidOut| {
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label("fitted")
            .expect("the fitted child is published");
        tree.window_bounds(&node).expect("the node has bounds").x0
    };
    assert_eq!(
        left(&laid),
        0.0,
        "LTR: the child's node is at the left edge"
    );

    laid.pump_widget(Directionality::new(TextDirection::Rtl, fitted()));
    laid.tick();
    assert_eq!(
        left(&laid),
        160.0,
        "RTL: the child's node moved to the right edge"
    );
}

/// A fit change that only moves the fitted transform moves the child's
/// published semantics with its pixels: `Contain` scales the 40×40 child to
/// fill the 200×200 box, `None` leaves it at its size, centred.
pub(crate) fn a_fitted_child_moved_by_a_fit_change_moves_its_semantics() {
    use flui_painting::BoxFit;
    use flui_widgets::{FittedBox, Semantics};

    let fitted = |fit| {
        FittedBox::new().fit(fit).child(
            Semantics::new()
                .label("fitted")
                .child(SizedBox::new(40.0, 40.0)),
        )
    };
    let mut laid = lay_out(fitted(BoxFit::Contain), tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let bounds = |laid: &crate::common::LaidOut| {
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label("fitted")
            .expect("the fitted child is published");
        let rect = tree.window_bounds(&node).expect("the node has bounds");
        (rect.x0, rect.x1)
    };
    assert_eq!(
        bounds(&laid),
        (0.0, 200.0),
        "Contain: the child fills the box"
    );

    laid.pump_widget(fitted(BoxFit::None));
    laid.tick();
    assert_eq!(
        bounds(&laid),
        (80.0, 120.0),
        "None: the child keeps its size, centred"
    );
}
