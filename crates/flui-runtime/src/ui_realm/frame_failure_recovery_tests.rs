use std::any::TypeId;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use flui_foundation::{ElementId, PresentationAddress};
use flui_types::Size;
use flui_view::{IntoView, StatelessView, View, element::ElementKind};
use flui_widgets::{Column, ListView, SizedBox};

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

fn real_recovery_location() -> RecoveredAt {
    let realm = UiRealm::for_test();
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&PanicsOnceOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            message: "location fixture",
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    let _presented = render_attempt(&realm, &mut backend);
    let observed = observed.lock().expect("failure collector mutex");
    let ObservedKind::Recovered { at, .. } = observed[0].kind else {
        panic!("expected recovered panic")
    };
    at
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
struct PanicsOnceWithNonStringPayload {
    should_panic: Rc<Cell<bool>>,
}

impl StatelessView for PanicsOnceWithNonStringPayload {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        if self.should_panic.replace(false) {
            std::panic::panic_any(41_u8);
        }
        SizedBox::new(10.0, 10.0)
    }
}

impl View for PanicsOnceWithNonStringPayload {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct RecordsElementThenPanicsOnBuild {
    should_panic: Rc<Cell<bool>>,
    observed_element_id: Rc<Cell<Option<ElementId>>>,
}

impl StatelessView for RecordsElementThenPanicsOnBuild {
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        self.observed_element_id.set(Some(ctx.element_id()));
        assert!(
            !self.should_panic.replace(false),
            "BUG: production adapter classification"
        );
        SizedBox::new(10.0, 10.0)
    }
}

impl View for RecordsElementThenPanicsOnBuild {
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

#[test]
fn recovery_report_types_are_exported_from_the_crate_root() {
    let disposition = FailureDisposition::Contained;
    let hook = LifecycleHook::Build;
    let at: Option<RecoveredAt> = None;
    assert_eq!(disposition, FailureDisposition::Contained);
    assert_eq!(hook, LifecycleHook::Build);
    assert!(at.is_none());
}

#[test]
fn converter_preserves_every_hook_attribution_and_input_order() {
    let hooks = [
        LifecycleHook::Build,
        LifecycleHook::InitState,
        LifecycleHook::DidChangeDependencies,
        LifecycleHook::Mount,
        LifecycleHook::Activate,
        LifecycleHook::Update,
        LifecycleHook::Deactivate,
        LifecycleHook::Dispose,
        LifecycleHook::UnmountRenderObject,
        LifecycleHook::LazyIndexLookup,
    ];
    let location = real_recovery_location();
    let converted: Vec<_> = hooks
        .iter()
        .enumerate()
        .map(|(index, hook)| {
            FrameFailureDetail::Verbatim.recovered_panic_kind_from_parts(
                location,
                TypeId::of::<PanicsOnceOnBuild>(),
                *hook,
                Some(format!("message-{index}").into_boxed_str()),
                index % 2 == 0,
            )
        })
        .collect();

    for (index, kind) in converted.iter().enumerate() {
        let FrameFailureKind::RecoveredPanic {
            at,
            view_type_id,
            hook,
            message,
            internal_invariant,
        } = kind
        else {
            panic!("converter returned a non-recovery kind")
        };
        assert_eq!(*at, location);
        assert_eq!(*view_type_id, TypeId::of::<PanicsOnceOnBuild>());
        assert_eq!(*hook, hooks[index]);
        assert_eq!(
            message,
            &PanicText::Verbatim(format!("message-{index}").into())
        );
        assert_eq!(*internal_invariant, index % 2 == 0);
    }
}

#[test]
fn recovered_message_policy_is_exact_and_never_carries_error_details() {
    let location = real_recovery_location();
    let convert = |detail: FrameFailureDetail, payload_text: Option<Box<str>>| {
        detail.recovered_panic_kind_from_parts(
            location,
            TypeId::of::<PanicsOnceOnBuild>(),
            LifecycleHook::Build,
            payload_text,
            false,
        )
    };
    let verbatim = convert(
        FrameFailureDetail::Verbatim,
        Some("ошибка\0with unicode".into()),
    );
    let redacted = convert(
        FrameFailureDetail::Redacted,
        Some("must-not-survive".into()),
    );
    let non_string = convert(FrameFailureDetail::Verbatim, None);
    let FrameFailureKind::RecoveredPanic { message, .. } = verbatim else {
        panic!("expected recovered panic")
    };
    assert_eq!(message, PanicText::Verbatim("ошибка\0with unicode".into()));
    let FrameFailureKind::RecoveredPanic { message, .. } = redacted else {
        panic!("expected recovered panic")
    };
    assert_eq!(message, PanicText::Redacted);
    assert!(!format!("{message:?}").contains("must-not-survive"));
    let FrameFailureKind::RecoveredPanic { message, .. } = non_string else {
        panic!("expected recovered panic")
    };
    assert_eq!(message, PanicText::Redacted);
}

#[test]
fn real_build_recovery_is_reported_once_in_the_same_attempt() {
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

#[test]
fn explicit_verbatim_keeps_a_non_string_lifecycle_payload_redacted() {
    let realm = UiRealm::for_test();
    realm.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&PanicsOnceWithNonStringPayload {
            should_panic: Rc::new(Cell::new(true)),
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    assert!(render_attempt(&realm, &mut backend));

    let observed = observed.lock().expect("failure collector mutex");
    assert_eq!(observed.len(), 1, "the recovery is delivered exactly once");
    assert_eq!(observed[0].disposition, FailureDisposition::Contained);
    let ObservedKind::Recovered {
        hook,
        message,
        internal_invariant,
        ..
    } = &observed[0].kind
    else {
        panic!("expected recovered lifecycle panic")
    };
    assert_eq!(*hook, LifecycleHook::Build);
    assert_eq!(message, &PanicText::Redacted);
    assert!(!internal_invariant);
}

#[cfg(not(debug_assertions))]
#[test]
fn release_default_keeps_a_non_string_lifecycle_payload_redacted() {
    let realm = UiRealm::for_test();
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&PanicsOnceWithNonStringPayload {
            should_panic: Rc::new(Cell::new(true)),
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    assert!(render_attempt(&realm, &mut backend));

    let observed = observed.lock().expect("failure collector mutex");
    assert_eq!(observed.len(), 1, "the recovery is delivered exactly once");
    let ObservedKind::Recovered { message, .. } = &observed[0].kind else {
        panic!("expected recovered lifecycle panic")
    };
    assert_eq!(message, &PanicText::Redacted);
}

#[test]
fn real_bug_prefixed_recovery_preserves_classification_and_full_attribution() {
    let realm = UiRealm::for_test();
    realm.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let observed = install_collecting_handler(&realm);
    let observed_element_id = Rc::new(Cell::new(None));
    realm
        .attach_root_widget(&RecordsElementThenPanicsOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            observed_element_id: Rc::clone(&observed_element_id),
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    let _presented = render_attempt(&realm, &mut backend);

    let observed = observed.lock().expect("failure collector mutex");
    assert_eq!(observed.len(), 1);
    let failure = &observed[0];
    assert_eq!(failure.address.realm_id, realm.realm_id());
    assert_eq!(failure.address.presentation_id, realm.presentation_id());
    assert_eq!(failure.disposition, FailureDisposition::Contained);
    let ObservedKind::Recovered {
        at,
        view_type_id,
        hook,
        message,
        internal_invariant,
    } = &failure.kind
    else {
        panic!("expected recovered lifecycle panic")
    };
    let panicking_element_id = observed_element_id
        .get()
        .expect("the panicking build records its own element id");
    assert!(matches!(
        at,
        RecoveredAt::Element {
            element,
            parent: None,
            ..
        } if *element == panicking_element_id
    ));
    assert_eq!(
        *view_type_id,
        TypeId::of::<RecordsElementThenPanicsOnBuild>()
    );
    assert_eq!(*hook, LifecycleHook::Build);
    assert_eq!(
        message,
        &PanicText::Verbatim("BUG: production adapter classification".into())
    );
    assert!(*internal_invariant);
}

#[test]
fn two_real_recoveries_from_one_attempt_are_delivered_in_production_queue_order() {
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

#[test]
fn real_lazy_recovery_is_reported_in_the_servicing_attempt_without_duplicates() {
    let realm = UiRealm::for_test();
    realm.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&ListView::builder(
            1,
            20.0,
            |_| -> Option<flui_view::BoxedView> { panic!("lazy builder recovery") },
        ))
        .expect("lazy root attaches");
    let mut backend = ScriptedSink::always_presents();

    let _servicing_attempt_presented = render_attempt(&realm, &mut backend);
    let reports_after_service = observed.lock().expect("failure collector mutex").len();
    let _following_attempt_presented = render_attempt(&realm, &mut backend);

    let observed = observed.lock().expect("failure collector mutex");
    assert_eq!(
        reports_after_service, 1,
        "lazy recovery must drain in its own attempt"
    );
    assert_eq!(
        observed.len(),
        1,
        "the following attempt must not drain it again"
    );
    match &observed[0].kind {
        ObservedKind::Recovered {
            at, hook, message, ..
        } => {
            assert_eq!(*hook, LifecycleHook::Build);
            assert_eq!(
                message,
                &PanicText::Verbatim("lazy builder recovery".into())
            );
            assert!(matches!(
                at,
                RecoveredAt::LazyDelegate { index: Some(0), .. }
            ));
        }
        other => panic!("expected lazy recovery, got {other:?}"),
    }
}

#[test]
fn recovered_reports_precede_tail_and_scene_frame_drops() {
    for phase in [SegmentPhase::Tail, SegmentPhase::Scene] {
        let realm = UiRealm::for_test();
        let observed = install_collecting_handler(&realm);
        realm
            .attach_root_widget(&PanicsOnceOnBuild {
                should_panic: Rc::new(Cell::new(true)),
                message: "recovered before terminal",
            })
            .expect("root attaches");
        realm
            .presentations
            .primary()
            .set_segment_probe(phase, Some(Box::new(|| panic!("terminal probe"))));
        let mut backend = ScriptedSink::always_presents();
        assert!(!render_attempt(&realm, &mut backend));

        let observed = observed.lock().expect("failure collector mutex");
        assert_eq!(observed.len(), 2, "both reports must be delivered");
        assert_eq!(observed[0].disposition, FailureDisposition::Contained);
        assert_eq!(observed[0].consecutive_failures, 0);
        assert_eq!(observed[1].disposition, FailureDisposition::FrameDropped);
        assert_eq!(observed[1].consecutive_failures, 1);
        assert_eq!(observed[1].kind, ObservedKind::Segment { phase });
    }
}

#[test]
fn contained_report_exposes_the_existing_streak_until_clean_delivery_finishes() {
    let realm = UiRealm::for_test();
    let observed = install_collecting_handler(&realm);
    realm.presentations.primary().note_frame_failure();
    realm
        .attach_root_widget(&PanicsOnceOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            message: "recovery while streak is live",
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    assert!(render_attempt(&realm, &mut backend));
    assert_eq!(
        observed.lock().expect("failure collector mutex")[0].consecutive_failures,
        1
    );

    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(|| panic!("drop after clean reset"))),
    );
    assert!(!render_attempt(&realm, &mut backend));
    let observed = observed.lock().expect("failure collector mutex");
    assert_eq!(observed[1].disposition, FailureDisposition::FrameDropped);
    assert_eq!(observed[1].consecutive_failures, 1);
}

#[derive(Debug)]
struct PanicOnLayoutBox;

impl flui_foundation::Diagnosticable for PanicOnLayoutBox {}

impl flui_rendering::traits::RenderBox for PanicOnLayoutBox {
    type Arity = flui_rendering::prelude::Leaf;
    type ParentData = flui_rendering::prelude::BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> Size {
        panic!("pipeline layout panic")
    }
}

#[test]
fn recovered_report_precedes_a_real_pipeline_drop() {
    let realm = UiRealm::for_test();
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&PanicsOnceOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            message: "recovered before pipeline",
        })
        .expect("root attaches");
    realm.pipeline_for_test().with_mut(|owner| {
        let root_id = owner.insert(Box::new(PanicOnLayoutBox)
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        owner.set_root_id(Some(root_id));
    });
    let mut backend = ScriptedSink::always_presents();
    assert!(!render_attempt(&realm, &mut backend));

    let observed = observed.lock().expect("failure collector mutex");
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0].disposition, FailureDisposition::Contained);
    assert_eq!(observed[1].disposition, FailureDisposition::FrameDropped);
    assert_eq!(observed[1].kind, ObservedKind::Pipeline);
}

#[test]
fn recovery_reports_keep_their_presentation_addresses_isolated() {
    let mut realm = UiRealm::for_test();
    let first = realm.presentation_id();
    let second = realm.install_second_presentation_for_test();
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&PanicsOnceOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            message: "first recovery",
        })
        .expect("first root attaches");
    realm
        .attach_root_widget_to_for_test(
            second,
            &PanicsOnceOnBuild {
                should_panic: Rc::new(Cell::new(true)),
                message: "second recovery",
            },
        )
        .expect("second root attaches");
    let mut backend = ScriptedSink::always_presents();
    assert!(render_attempt(&realm, &mut backend));

    let observed = observed.lock().expect("failure collector mutex");
    let addresses: Vec<_> = observed
        .iter()
        .map(|failure| failure.address.presentation_id)
        .collect();
    assert_eq!(addresses, vec![first, second]);
    assert!(
        observed
            .iter()
            .all(|failure| failure.address.realm_id == realm.realm_id())
    );
}

#[test]
fn empty_drain_emits_nothing() {
    let realm = UiRealm::for_test();
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    assert!(render_attempt(&realm, &mut backend));
    assert!(observed.lock().expect("failure collector mutex").is_empty());
    assert!(realm.widgets().take_recovered_panics().is_empty());
}

#[test]
fn default_recovery_privacy_matches_the_build_profile() {
    let realm = UiRealm::for_test();
    let observed = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&PanicsOnceOnBuild {
            should_panic: Rc::new(Cell::new(true)),
            message: "profile-sensitive recovery",
        })
        .expect("root attaches");
    let mut backend = ScriptedSink::always_presents();
    assert!(render_attempt(&realm, &mut backend));

    let observed = observed.lock().expect("failure collector mutex");
    let ObservedKind::Recovered { message, .. } = &observed[0].kind else {
        panic!("expected recovered panic")
    };
    #[cfg(debug_assertions)]
    assert_eq!(
        message,
        &PanicText::Verbatim("profile-sensitive recovery".into())
    );
    #[cfg(not(debug_assertions))]
    assert_eq!(message, &PanicText::Redacted);
}

#[test]
fn panicking_handler_does_not_escape_or_duplicate_recovery() {
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
