use std::any::TypeId;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use flui_foundation::PresentationAddress;
use flui_view::{IntoView, StatelessView, View, element::ElementKind};
use flui_widgets::{Column, SizedBox};

use super::*;
use crate::frame_failure::{FailureDisposition, FrameFailureHandler, FrameFailureKind, PanicText};
use crate::testing::ScriptedSink;
use flui_view::{LifecycleHook, RecoveredAt};

#[derive(Debug, Clone, PartialEq)]
struct ObservedFailure {
    address: PresentationAddress,
    disposition: FailureDisposition,
    consecutive_failures: u32,
    kind: ObservedKind,
}

#[derive(Debug, Clone, PartialEq)]
enum ObservedKind {
    Recovered {
        at: RecoveredAt,
        view_type_id: TypeId,
        hook: LifecycleHook,
        message: PanicText,
        internal_invariant: bool,
    },
    Segment {
        phase: SegmentPhase,
    },
    Pipeline,
    Callback,
}

fn install_collecting_handler(realm: &UiRealm) -> Arc<Mutex<Vec<ObservedFailure>>> {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let handler_observed = Arc::clone(&observed);
    realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        let kind = match &report.kind {
            FrameFailureKind::RecoveredPanic {
                at,
                view_type_id,
                hook,
                message,
                internal_invariant,
            } => ObservedKind::Recovered {
                at: *at,
                view_type_id: *view_type_id,
                hook: *hook,
                message: message.clone(),
                internal_invariant: *internal_invariant,
            },
            FrameFailureKind::SegmentPanic { phase, .. } => ObservedKind::Segment { phase: *phase },
            FrameFailureKind::Pipeline { .. } => ObservedKind::Pipeline,
            FrameFailureKind::CallbackPanic { .. } => ObservedKind::Callback,
        };
        handler_observed
            .lock()
            .expect("failure collector mutex")
            .push(ObservedFailure {
                address: report.address,
                disposition: report.disposition,
                consecutive_failures: report.consecutive_failures,
                kind,
            });
    })));
    observed
}

fn render_attempt(realm: &UiRealm, backend: &mut ScriptedSink) -> bool {
    realm.request_redraw();
    realm.mark_rendered();
    catch_unwind(AssertUnwindSafe(|| realm.render_frame(backend)))
        .expect("the realm boundary must contain the intentional panic")
}

#[derive(Clone)]
struct PanicsOnceOnBuild {
    should_panic: Rc<Cell<bool>>,
    message: &'static str,
}

impl StatelessView for PanicsOnceOnBuild {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        assert!(!self.should_panic.replace(false), "{}", self.message);
        SizedBox::new(10.0, 10.0)
    }
}

impl View for PanicsOnceOnBuild {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct TwoPanickingChildren;

impl StatelessView for TwoPanickingChildren {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        Column::new((
            PanicsOnceOnBuild {
                should_panic: Rc::new(Cell::new(true)),
                message: "first recovered child",
            },
            PanicsOnceOnBuild {
                should_panic: Rc::new(Cell::new(true)),
                message: "second recovered child",
            },
        ))
    }
}

impl View for TwoPanickingChildren {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

pub(crate) fn real_build_recovery_is_reported_once_in_the_same_attempt() {
    let realm = UiRealm::for_test();
    realm.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&PanicsOnceOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            message: "same-attempt recovery",
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    let _first_presented = render_attempt(&realm, &mut backend);
    let _second_presented = render_attempt(&realm, &mut backend);

    let observed = observed.lock().expect("failure collector mutex");
    assert_eq!(observed.len(), 1, "the drain must not duplicate a record");
    assert_eq!(observed[0].disposition, FailureDisposition::Contained);
    assert_eq!(observed[0].consecutive_failures, 0);
    match &observed[0].kind {
        ObservedKind::Recovered { hook, message, .. } => {
            assert_eq!(*hook, LifecycleHook::Build);
            assert_eq!(
                message,
                &PanicText::Verbatim("same-attempt recovery".into())
            );
        }
        other => panic!("expected recovered build panic, got {other:?}"),
    }
}

pub(crate) fn two_real_recoveries_from_one_attempt_are_delivered_in_production_queue_order() {
    let realm = UiRealm::for_test();
    realm.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&TwoPanickingChildren)
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    let _presented = render_attempt(&realm, &mut backend);

    let observed = observed.lock().expect("failure collector mutex");
    let messages: Vec<_> = observed
        .iter()
        .map(|failure| match &failure.kind {
            ObservedKind::Recovered { message, .. } => message.clone(),
            other => panic!("expected only recovered reports, got {other:?}"),
        })
        .collect();
    assert_eq!(
        messages,
        vec![
            PanicText::Verbatim("first recovered child".into()),
            PanicText::Verbatim("second recovered child".into()),
        ]
    );
    assert!(
        observed
            .iter()
            .all(|failure| failure.disposition == FailureDisposition::Contained)
    );
}

pub(crate) fn panicking_handler_does_not_escape_or_duplicate_recovery() {
    let realm = UiRealm::for_test();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler_calls = Arc::clone(&calls);
    realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |_| {
        handler_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        panic!("handler panic")
    })));
    realm
        .attach_root_widget(&PanicsOnceOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            message: "contained producer",
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    let _first_presented = render_attempt(&realm, &mut backend);
    let _second_presented = render_attempt(&realm, &mut backend);
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
}
