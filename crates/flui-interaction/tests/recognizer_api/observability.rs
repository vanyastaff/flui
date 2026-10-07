//! Stable tracing event kinds observed through public recognizer dispatch.

use std::{
    fmt,
    rc::Rc,
    sync::{Arc, Mutex},
};

use flui_foundation::geometry::Offset;
use flui_interaction::{
    EagerGestureRecognizer, GestureRecognizer, LongPressGestureRecognizer, MultiDragAxis,
    MultiDragGestureRecognizer, PointerId, TapAndDragGestureRecognizer, TapGestureRecognizer,
    arena::GestureArena,
    events::{PointerType, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id},
    routing::PointerDispatch,
};

#[derive(Default)]
struct EventKindVisitor(Option<String>);

impl tracing::field::Visit for EventKindVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "event" {
            self.0 = Some(format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "event" {
            self.0 = Some(value.to_owned());
        }
    }
}

struct RecognizerEvents(Arc<Mutex<Vec<String>>>);

impl RecognizerEvents {
    fn append(&self, visitor: EventKindVisitor) {
        if let Some(kind) = visitor.0 {
            self.0
                .lock()
                .expect("test event collector is not poisoned")
                .push(kind);
        }
    }
}

impl tracing::Subscriber for RecognizerEvents {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata
            .target()
            .starts_with("flui_interaction::recognizers")
    }

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut visitor = EventKindVisitor::default();
        attributes.record(&mut visitor);
        self.append(visitor);
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, record: &tracing::span::Record<'_>) {
        let mut visitor = EventKindVisitor::default();
        record.record(&mut visitor);
        self.append(visitor);
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut visitor = EventKindVisitor::default();
        event.record(&mut visitor);
        self.append(visitor);
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[derive(Clone, Copy)]
enum RecognizerKind {
    Tap,
    Eager,
    LongPress,
    MultiDrag,
}

fn dispatch_contact(recognizer: &dyn GestureRecognizer, arena: &GestureArena) {
    let pointer = PointerId::new(1).expect("nonzero pointer");
    let down = make_down_event_for_id(pointer, Offset::ZERO, PointerType::Touch);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(pointer);
    arena.drain_deferred_resolutions();
    let movement = make_move_event_for_id(pointer, Offset::new(2.0, 0.0), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    let up = make_up_event_for_id(pointer, Offset::new(2.0, 0.0), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
}

fn observes(kind: RecognizerKind, expected: &str) {
    let events = Arc::new(Mutex::new(Vec::new()));
    tracing::subscriber::with_default(RecognizerEvents(Arc::clone(&events)), || {
        // A surviving production emitter proves the subscriber sees the stable
        // field before checking each independently affected recognizer.
        let control_arena = GestureArena::new();
        let control = TapAndDragGestureRecognizer::builder(control_arena.clone()).build();
        dispatch_contact(&*control, &control_arena);
        let collected =
            std::mem::take(&mut *events.lock().expect("test event collector is not poisoned"));
        assert!(
            collected.iter().any(|value| value == expected),
            "production control did not emit {expected}: {collected:?}"
        );

        let arena = GestureArena::new();
        let recognizer: Rc<dyn GestureRecognizer> = match kind {
            RecognizerKind::Tap => TapGestureRecognizer::builder(arena.clone()).build(),
            RecognizerKind::Eager => EagerGestureRecognizer::builder(arena.clone()).build(),
            RecognizerKind::LongPress => LongPressGestureRecognizer::builder(arena.clone()).build(),
            RecognizerKind::MultiDrag => {
                MultiDragGestureRecognizer::builder(arena.clone(), MultiDragAxis::Free).build()
            }
        };
        dispatch_contact(&*recognizer, &arena);
        let collected = events
            .lock()
            .expect("test event collector is not poisoned")
            .clone();
        assert!(
            collected.iter().any(|value| value == expected),
            "recognizer did not emit stable {expected}: {collected:?}"
        );
    });
}

fn tap_reports_admission() {
    observes(RecognizerKind::Tap, "recognizer_added");
}
fn tap_reports_dispatch() {
    observes(RecognizerKind::Tap, "event_received");
}
fn eager_reports_admission() {
    observes(RecognizerKind::Eager, "recognizer_added");
}
fn eager_reports_dispatch() {
    observes(RecognizerKind::Eager, "event_received");
}
fn long_press_reports_admission() {
    observes(RecognizerKind::LongPress, "recognizer_added");
}
fn long_press_reports_dispatch() {
    observes(RecognizerKind::LongPress, "event_received");
}
fn multi_drag_reports_admission() {
    observes(RecognizerKind::MultiDrag, "recognizer_added");
}
fn multi_drag_reports_dispatch() {
    observes(RecognizerKind::MultiDrag, "event_received");
}

#[test]
fn stable_recognizer_observability_kinds_reach_the_subscriber() {
    let cases: &[(&str, fn())] = &[
        ("tap admission", tap_reports_admission),
        ("tap dispatch", tap_reports_dispatch),
        ("eager admission", eager_reports_admission),
        ("eager dispatch", eager_reports_dispatch),
        ("long press admission", long_press_reports_admission),
        ("long press dispatch", long_press_reports_dispatch),
        ("multi drag admission", multi_drag_reports_admission),
        ("multi drag dispatch", multi_drag_reports_dispatch),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("case {name}");
            failures.push(payload);
        }
    }
    if let Some(first) = failures.into_iter().next() {
        std::panic::resume_unwind(first);
    }
}
