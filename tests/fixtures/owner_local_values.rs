//! Owner-local values fit every position a consumer reaches that runs on the
//! UI owner: curves, scroll physics, widget-state resolvers, hit-test
//! metadata, painters and layout delegates. Each value captures an
//! `Rc<Cell<_>>`, so a `Send` bound returning to any of these positions stops
//! this consumer from compiling. It names only paths `flui` and `flui-sdk`
//! share, so the same source checks both.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui::animation::{Curve, MotionSpec, Simulation};
use flui::foundation::geometry::{Offset, Size};
use flui::painting::Canvas;
use flui::rendering::{BoxConstraints, RenderResult};
use flui::view::BoxedView;
use flui::widgets::{
    AnimatedSwitcher, CustomMultiChildLayout, CustomPaint, CustomPainter, CustomSingleChildLayout,
    Flow, FlowDelegate, FlowPaintingContext, MetaData, MultiChildLayoutContext,
    MultiChildLayoutDelegate, ScrollMetrics, ScrollPhysics, Scrollable, SingleChildLayoutDelegate,
    WidgetStateProperty, WidgetStates,
};

/// Owner-local state every value below reads.
type Shared = Rc<Cell<u32>>;

/// Squares progress and counts evaluations.
struct CountingCurve(Shared);

impl Curve for CountingCurve {
    fn transform(&self, t: f64) -> f64 {
        self.0.set(self.0.get() + 1);
        t * t
    }
}

#[derive(Debug)]
struct ClampingWithState(Shared);

impl ScrollPhysics for ClampingWithState {
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f64) -> f64 {
        self.0.set(self.0.get() + 1);
        proposed_pixels.clamp(metrics.min_scroll_extent, metrics.max_scroll_extent)
    }

    fn create_ballistic_simulation(
        &self,
        _metrics: &ScrollMetrics,
        _velocity_px_per_sec: f64,
    ) -> Option<Box<dyn Simulation>> {
        None
    }
}

#[derive(Debug)]
struct PainterWithState(Shared);

impl CustomPainter for PainterWithState {
    fn paint(&self, _canvas: &mut Canvas, _size: Size) {
        self.0.set(self.0.get() + 1);
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct FlowWithState(Shared);

impl FlowDelegate for FlowWithState {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.smallest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        constraints.loosen()
    }

    fn paint_children(&self, _context: &mut FlowPaintingContext<'_, '_>) {
        self.0.set(self.0.get() + 1);
    }

    fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct SingleWithState(Shared);

impl SingleChildLayoutDelegate for SingleWithState {
    fn get_position_for_child(&self, _size: Size, _child_size: Size) -> Offset {
        self.0.set(self.0.get() + 1);
        Offset::ZERO
    }

    fn should_relayout(&self, _old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct MultiWithState(Shared);

impl MultiChildLayoutDelegate for MultiWithState {
    fn perform_layout(
        &self,
        _context: &mut dyn MultiChildLayoutContext,
        _size: Size,
    ) -> RenderResult<()> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }

    fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn owner_local_values_fit_every_flip_position() {
    let shared: Shared = Rc::new(Cell::new(0));

    // Curves: the motion spec, and a widget setter.
    let spec = MotionSpec::default().with_curve(CountingCurve(shared.clone()));
    let MotionSpec::Curve { curve, .. } = &spec else {
        panic!("with_curve makes curve motion");
    };
    assert_eq!(curve.transform(0.5), 0.25);
    assert_eq!(shared.get(), 1, "the erased curve evaluates the owner-local one");
    let _switcher = AnimatedSwitcher::new(Duration::from_millis(100))
        .switch_in_curve(CountingCurve(shared.clone()));

    // Scroll physics.
    let _scrollable = Scrollable::new().physics(Rc::new(ClampingWithState(shared.clone())));

    // A widget-state resolver.
    let observed = shared.clone();
    let property = WidgetStateProperty::resolve_with(move |_states: &WidgetStates| observed.get());
    assert_eq!(property.resolve(&WidgetStates::NONE), 1);

    // Hit-test metadata, through the widget that attaches it.
    let _meta = MetaData::new(shared.clone());

    // Painters and layout delegates, taken by value.
    let _paint = CustomPaint::new()
        .painter(PainterWithState(shared.clone()))
        .foreground_painter(PainterWithState(shared.clone()));
    let _flow = Flow::new(FlowWithState(shared.clone()), Vec::<BoxedView>::new());
    let _single = CustomSingleChildLayout::new(SingleWithState(shared.clone()));
    let _multi = CustomMultiChildLayout::new(MultiWithState(shared), Vec::<BoxedView>::new());
}
