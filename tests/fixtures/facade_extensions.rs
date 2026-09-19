#![cfg(test)]

use flui::geometry::px;
use flui::painting::{Canvas, CustomPainter, DrawOp, Paint};
use flui::prelude::*;
use flui::rendering::{
    BoxConstraints, BoxDryLayoutCtx, BoxLayoutContext, BoxParentData, BoxProtocol, Leaf, PaintCx,
    RenderBox, RenderUpdateImpact, Single,
};
use flui::testing::widgets::{lay_out, loose};
use flui::types::{Point, Rect, Size};
use flui::view::{RenderObjectContext, RenderView};
use flui::widgets::AnimatedBuilder;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Debug)]
struct RectanglePainter;

impl CustomPainter for RectanglePainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        canvas.draw_rect(
            Rect::from_origin_size(Point::ZERO, size),
            &Paint::fill(Color::rgb(10, 20, 30)),
        );
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn repaint(&self) -> Option<Arc<dyn flui::foundation::Listenable>> {
        None
    }

    fn semantics_builder(&self) -> Option<flui::painting::SemanticsBuilder> {
        None
    }
}

#[test]
fn custom_painter_records_a_rectangle() {
    let mut canvas = Canvas::new();
    RectanglePainter.paint(&mut canvas, Size::new(px(24.0), px(16.0)));
    let recording = canvas.finish();
    assert_eq!(recording.commands().len(), 1);
    let DrawOp::Rect { rect, paint } = &recording.commands()[0].op else {
        panic!("painter must record a rectangle");
    };
    assert_eq!(
        *rect,
        Rect::from_origin_size(Point::ZERO, Size::new(px(24.0), px(16.0)))
    );
    assert_eq!(paint.color, Color::rgb(10, 20, 30));
}

#[derive(Debug, flui::Diagnosticable)]
struct SolidLeaf {
    paints: Arc<AtomicUsize>,
}

impl RenderBox for SolidLeaf {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        ctx.constrain(Size::new(px(24.0), px(16.0)))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        constraints.constrain(Size::new(px(24.0), px(16.0)))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        let size = ctx.size();
        RectanglePainter.paint(ctx.canvas(), size);
        self.paints.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct LeafView {
    paints: Arc<AtomicUsize>,
}

impl RenderView for LeafView {
    type Protocol = BoxProtocol;
    type RenderObject = SolidLeaf;

    fn create_render_object(&self, _: &RenderObjectContext<'_>) -> SolidLeaf {
        SolidLeaf {
            paints: self.paints.clone(),
        }
    }

    fn update_render_object(
        &self,
        _: &RenderObjectContext<'_>,
        _: &mut SolidLeaf,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

flui::view::impl_render_view!(LeafView);

#[test]
fn custom_render_view_mounts_lays_out_and_paints() {
    let paints = Arc::new(AtomicUsize::new(0));
    let tree = lay_out(
        LeafView {
            paints: paints.clone(),
        },
        loose(100.0),
    );
    assert_eq!(tree.size(tree.root()), Size::new(px(24.0), px(16.0)));
    assert_eq!(paints.load(Ordering::SeqCst), 1);
}

#[derive(Debug, flui::Diagnosticable)]
struct TransparentProxy {
    has_child: bool,
}

impl RenderBox for TransparentProxy {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui::rendering::forward_single_child_box_layout!();
    flui::rendering::forward_single_child_box_queries!();
    flui::rendering::forward_single_child_box_hit_test!();

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }
}

#[test]
fn proxy_macros_forward_live_and_dry_layout() {
    use flui::testing::rendering::{BoxQueryRun, Probe, RenderTester, box_node};
    let mut run = RenderTester::mount(box_node(TransparentProxy { has_child: false }).child(
        box_node(SolidLeaf {
            paints: Arc::new(AtomicUsize::new(0)),
        }),
    ))
    .with_constraints(loose(100.0))
    .run_layout();
    let root = run.root();
    assert_eq!(run.box_geometry(root), Size::new(px(24.0), px(16.0)));
    let forced = Size::new(px(37.0), px(19.0));
    assert_eq!(run.dry_layout(root, BoxConstraints::tight(forced)), forced);
}

#[test]
fn gesture_recognizer_uses_headless_virtual_time() {
    use flui::interaction::{
        GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
    };
    use flui::types::Offset;
    let mut binding = flui::testing::HeadlessBinding::new();
    let fired = Arc::new(AtomicUsize::new(0));
    let callback = fired.clone();
    let recognizer = LongPressGestureRecognizer::with_settings(
        binding.arena().clone(),
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
    )
    .with_on_long_press_start(move |_details: flui::interaction::LongPressStartDetails| {
        callback.fetch_add(1, Ordering::SeqCst);
    });
    let position = Offset::new(px(8.0), px(8.0));
    recognizer.add_pointer(
        PointerId::new(1).expect("nonzero pointer"),
        position,
        position,
    );
    binding.pump_frame(Duration::from_millis(300));
    assert_eq!(fired.load(Ordering::SeqCst), 0);
    binding.pump_frame(Duration::from_millis(250));
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

#[test]
fn pointer_input_schedules_a_widget_rebuild() {
    let changed = Arc::new(flui::foundation::ChangeNotifier::new());
    let value = Arc::new(AtomicUsize::new(0));
    let builds = Arc::new(AtomicUsize::new(0));
    let read = value.clone();
    let builds_in_callback = builds.clone();
    let notification = changed.clone();
    let count = value.clone();
    let view = AnimatedBuilder::new(changed, move || {
        builds_in_callback.fetch_add(1, Ordering::SeqCst);
        let notification = notification.clone();
        let count = count.clone();
        GestureDetector::new()
            .on_tap(move || {
                count.fetch_add(1, Ordering::SeqCst);
                notification.notify_listeners();
            })
            .behavior(HitTestBehavior::Opaque)
            .child(SizedBox::new(
                40.0 + read.load(Ordering::SeqCst) as f32,
                30.0,
            ))
    });
    let mut tree = lay_out(view, loose(100.0));
    let initial_builds = builds.load(Ordering::SeqCst);
    assert_eq!(tree.size(tree.root()).width, px(40.0));
    tree.dispatch_pointer_down(10.0, 10.0);
    tree.dispatch_pointer_up(10.0, 10.0);
    tree.tick();
    assert_eq!(value.load(Ordering::SeqCst), 1);
    assert!(builds.load(Ordering::SeqCst) > initial_builds);
    assert_eq!(tree.size(tree.root()).width, px(41.0));
}

#[test]
fn typed_drag_down_callback_is_available_from_the_widget_surface() {
    let downs = Arc::new(AtomicUsize::new(0));
    let observed = downs.clone();
    let tree = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_down(move |_details: flui::widgets::DragDownDetails| {
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .behavior(HitTestBehavior::Opaque)
            .child(SizedBox::new(40.0, 30.0)),
        loose(100.0),
    );
    tree.dispatch_pointer_down(10.0, 10.0);
    assert_eq!(downs.load(Ordering::SeqCst), 1);
    tree.dispatch_pointer_up(10.0, 10.0);
}

#[derive(Clone, Debug)]
struct DistanceRecognizer {
    base: flui::interaction::RecognizerBase,
    threshold: f32,
    accepted: Arc<AtomicUsize>,
    rejected: Arc<AtomicUsize>,
}

impl flui::interaction::CustomGestureRecognizer for DistanceRecognizer {
    fn on_arena_accept(&self, pointer: flui::interaction::PointerId) {
        assert_eq!(self.base.primary_pointer(), Some(pointer));
        self.accepted.fetch_add(1, Ordering::SeqCst);
    }

    fn on_arena_reject(&self, pointer: flui::interaction::PointerId) {
        assert_eq!(self.base.primary_pointer(), Some(pointer));
        self.rejected.fetch_add(1, Ordering::SeqCst);
    }
}

impl flui::interaction::GestureRecognizer for DistanceRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: flui::interaction::PointerId,
        position: flui::types::Offset,
        global_position: flui::types::Offset,
    ) {
        self.base
            .start_tracking(pointer, position, global_position, self);
    }

    fn handle_event(&self, dispatch: flui::widgets::PointerDispatch<'_>) {
        use flui::interaction::{PointerEvent, PointerEventExt};
        if let PointerEvent::Move(_) = dispatch.local {
            let origin = self.base.initial_position().expect("tracked pointer");
            if (dispatch.local.position().dx - origin.dx).get() >= self.threshold {
                self.base.accept_tracked();
            }
        }
    }

    fn dispose(&self) {
        self.base.stop_tracking();
        self.base.mark_disposed();
    }

    fn primary_pointer(&self) -> Option<flui::interaction::PointerId> {
        self.base.primary_pointer()
    }
}

#[test]
fn downstream_custom_recognizer_competes_in_the_arena() {
    use flui::interaction::{GestureArenaMember, GestureRecognizer, PointerId, RecognizerBase};
    use flui::testing::replay::{PointerPhase, ScriptedPointer};
    use flui::types::Offset;
    use flui::widgets::PointerDispatch;

    let binding = flui::testing::HeadlessBinding::new();
    let arena = binding.arena();
    let recognizer = |threshold| {
        Arc::new(DistanceRecognizer {
            base: RecognizerBase::new(arena.clone()),
            threshold,
            accepted: Arc::new(AtomicUsize::new(0)),
            rejected: Arc::new(AtomicUsize::new(0)),
        })
    };
    let first = recognizer(10.0);
    let second = recognizer(20.0);
    // The public custom-recognizer contract supplies the sealed arena-member
    // implementation. Registration below uses those exact Arc identities.
    let _member: Arc<dyn GestureArenaMember> = first.clone();
    let pointer = PointerId::new(11).expect("nonzero pointer");
    for candidate in [&first, &second] {
        candidate.add_pointer(pointer, Offset::ZERO, Offset::ZERO);
        assert_eq!(candidate.primary_pointer(), Some(pointer));
    }
    arena.close(pointer);

    let moved = |x| {
        ScriptedPointer::new(
            Duration::ZERO,
            pointer,
            PointerPhase::Move,
            Offset::new(px(x), px(0.0)),
        )
        .to_event()
    };
    let below_threshold = moved(5.0);
    for candidate in [&first, &second] {
        candidate.handle_event(PointerDispatch::at_root(&below_threshold));
        assert_eq!(candidate.accepted.load(Ordering::SeqCst), 0);
        assert_eq!(candidate.rejected.load(Ordering::SeqCst), 0);
    }

    let crossed_threshold = moved(15.0);
    first.handle_event(PointerDispatch::at_root(&crossed_threshold));
    assert_eq!(first.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(first.rejected.load(Ordering::SeqCst), 0);
    assert_eq!(second.accepted.load(Ordering::SeqCst), 0);
    assert_eq!(second.rejected.load(Ordering::SeqCst), 1);
    for candidate in [&first, &second] {
        candidate.dispose();
        assert_eq!(candidate.primary_pointer(), None);
    }
}
