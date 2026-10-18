//! The extension surface carries a recognizer the framework does not ship: it
//! is written against `flui_sdk` alone, mounted by `RawGestureDetector`, and
//! keeps its contact when the parent rebuilds in the middle of the gesture.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_sdk::foundation::ValueKey;
use flui_sdk::geometry::Size;
use flui_sdk::interaction::{
    ArenaMembership, CancelOutcome, GestureArenaMember, GestureContest, GestureRecognizer,
    MotionRole, PointerDispatch, PointerEvent, PointerEventExt, PointerKind, PointerKindSet,
    PrimaryContact,
};
use flui_sdk::painting::Color;
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::widgets::{
    ColoredBox, GestureRecognizerFactory, RawGestureDetector, RecognizerContext,
};
use flui_testing::widgets::{LaidOut, lay_out};

type Log = Rc<RefCell<Vec<String>>>;

/// The configuration a rebuild hands the live recognizer.
#[derive(Clone)]
struct Spec {
    label: &'static str,
    /// Distance from the Down position the contact must exceed to drag.
    threshold: f64,
    /// The device kinds whose contacts the recognizer admits.
    devices: PointerKindSet,
    log: Log,
}

/// Reports the distance from its Down position once a contact has moved
/// past the configured threshold.
struct ThresholdDrag {
    /// The label of the configuration that created this recognizer.
    born: &'static str,
    config: RefCell<Spec>,
    dragging: Cell<bool>,
    contact: PrimaryContact,
    settings: flui_sdk::interaction::GestureSettingsProvider,
}

impl ThresholdDrag {
    fn report(&self, event: &str) {
        let Spec { label, log, .. } = self.config.borrow().clone();
        log.borrow_mut()
            .push(format!("{}/{label}: {event}", self.born));
    }
}

impl GestureArenaMember for ThresholdDrag {
    fn accept_gesture(&self, _: GestureContest) {}

    fn reject_gesture(&self, _: GestureContest) {
        self.dragging.set(false);
        self.contact.withdraw();
    }
}

impl GestureRecognizer for ThresholdDrag {
    fn allowed_devices(&self) -> PointerKindSet {
        self.config.borrow().devices
    }

    fn add_pointer(&self, down: PointerDispatch<'_>) {
        let Some(kind) = down.local.pointer_kind() else {
            return;
        };
        let _ = self.contact.begin(down, &self.settings.snapshot_for(kind));
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if !dispatch
            .local
            .pointer_id()
            .is_some_and(|pointer| self.contact.tracks(pointer))
        {
            return;
        }
        match dispatch.local {
            PointerEvent::Move(_) => {
                let (Some(contact), Some(local), Some(global)) = (
                    self.contact.current(),
                    dispatch.local.position(),
                    dispatch.global.position(),
                ) else {
                    return;
                };
                let moved = local - contact.local;
                let distance = moved.dx.hypot(moved.dy);
                if !self.dragging.get() {
                    if distance <= self.config.borrow().threshold {
                        return;
                    }
                    self.dragging.set(true);
                    self.contact.claim_motion(global - contact.global);
                    if !self.dragging.get() {
                        return; // the arena gave the contact to another member
                    }
                }
                self.report(&format!("drag {distance}"));
            }
            PointerEvent::Up(_) => {
                if self.dragging.replace(false) {
                    self.report("end");
                }
                self.contact.finish();
            }
            PointerEvent::Cancel(_) => {
                self.cancel();
            }
            _ => {}
        }
    }

    fn cancel(&self) -> CancelOutcome {
        self.dragging.set(false);
        if self.contact.withdraw().is_some() {
            self.report("cancel");
            CancelOutcome::Cancelled
        } else {
            CancelOutcome::Idle
        }
    }
}

struct ThresholdDragFactory(Spec);

impl GestureRecognizerFactory for ThresholdDragFactory {
    type Recognizer = ThresholdDrag;

    fn create(&self, cx: &RecognizerContext) -> Rc<ThresholdDrag> {
        let arena = cx.arena().clone();
        Rc::new_cyclic(|this: &Weak<ThresholdDrag>| ThresholdDrag {
            born: self.0.label,
            config: RefCell::new(self.0.clone()),
            dragging: Cell::new(false),
            contact: PrimaryContact::new(
                ArenaMembership::new(arena, this.clone()).with_motion_role(MotionRole::Free),
            ),
            settings: cx.settings().clone(),
        })
    }

    fn update(&self, recognizer: &ThresholdDrag) {
        recognizer.config.replace(self.0.clone());
    }
}

fn spec(label: &'static str, threshold: f64, log: &Log) -> ThresholdDragFactory {
    ThresholdDragFactory(Spec {
        label,
        threshold,
        devices: PointerKindSet::all(),
        log: Rc::clone(log),
    })
}

fn surface() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

fn mount(detector: RawGestureDetector) -> LaidOut {
    lay_out(
        detector.child(surface()),
        BoxConstraints::tight(Size::new(100.0, 100.0)),
    )
}

fn entries(log: &Log) -> Vec<String> {
    log.borrow().clone()
}

/// A parent rebuild in the middle of a drag reconfigures the live recognizer:
/// the drag continues from the original Down under the new configuration
/// instead of being cancelled and lost to a fresh recognizer.
#[test]
fn a_rebuild_mid_gesture_keeps_the_drag() {
    let log = Log::default();
    let mut laid = mount(RawGestureDetector::new().recognizer(spec("first", 10.0, &log)));
    laid.dispatch_pointer_down(50.0, 10.0);
    laid.dispatch_pointer_move(50.0, 30.0);

    laid.pump_widget(
        RawGestureDetector::new()
            .recognizer(spec("second", 10.0, &log))
            .child(surface()),
    );
    laid.dispatch_pointer_move(50.0, 40.0);
    laid.dispatch_pointer_up(50.0, 40.0);

    assert_eq!(
        entries(&log),
        [
            "first/first: drag 20",
            "first/second: drag 30",
            "first/second: end"
        ]
    );
}

/// `update` reaches the live recognizer: a threshold lowered mid-contact
/// starts the drag on the next move.
#[test]
fn update_applies_the_new_configuration_to_the_live_recognizer() {
    let log = Log::default();
    let mut laid = mount(RawGestureDetector::new().recognizer(spec("far", 40.0, &log)));
    laid.dispatch_pointer_down(50.0, 10.0);
    laid.dispatch_pointer_move(50.0, 30.0);
    assert_eq!(entries(&log), Vec::<String>::new(), "20 is within 40");

    laid.pump_widget(
        RawGestureDetector::new()
            .recognizer(spec("near", 10.0, &log))
            .child(surface()),
    );
    laid.dispatch_pointer_move(50.0, 35.0);
    laid.dispatch_pointer_up(50.0, 35.0);

    assert_eq!(entries(&log), ["far/near: drag 25", "far/near: end"]);
}

/// A factory that leaves the list cancels its recognizer's contact.
#[test]
fn a_removed_factory_cancels_its_recognizer() {
    let log = Log::default();
    let mut laid = mount(RawGestureDetector::new().recognizer(spec("only", 10.0, &log)));
    laid.dispatch_pointer_down(50.0, 10.0);
    laid.dispatch_pointer_move(50.0, 30.0);

    laid.pump_widget(RawGestureDetector::new().child(surface()));
    laid.dispatch_pointer_move(50.0, 40.0);
    laid.dispatch_pointer_up(50.0, 40.0);

    assert_eq!(entries(&log), ["only/only: drag 20", "only/only: cancel"]);
}

/// Keys keep two recognizers of one factory type apart when the list is
/// reordered: each configuration reaches the recognizer its key created. The
/// `touch` recognizer refuses the mouse contact, so the mouse drag reaches
/// only the recognizer that holds the `mouse` configuration.
#[test]
fn keyed_factories_keep_their_recognizers_when_reordered() {
    let touch_only = |label, log: &Log| {
        let mut factory = spec(label, 10.0, log);
        factory.0.devices = PointerKindSet::of(PointerKind::Touch);
        factory
    };
    let log = Log::default();
    let mut laid = mount(
        RawGestureDetector::new()
            .recognizer_with_key(ValueKey::new("mouse"), spec("mouse", 10.0, &log))
            .recognizer_with_key(ValueKey::new("touch"), touch_only("touch", &log)),
    );

    laid.pump_widget(
        RawGestureDetector::new()
            .recognizer_with_key(ValueKey::new("touch"), touch_only("touch again", &log))
            .recognizer_with_key(ValueKey::new("mouse"), spec("mouse again", 10.0, &log))
            .child(surface()),
    );
    laid.dispatch_pointer_down(50.0, 10.0);
    laid.dispatch_pointer_move(50.0, 30.0);
    laid.dispatch_pointer_up(50.0, 30.0);

    assert_eq!(
        entries(&log),
        ["mouse/mouse again: drag 20", "mouse/mouse again: end"]
    );
}
