use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use flui_widgets::SizedBox;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

use super::UiRealm;
use crate::frame_failure::{
    FrameFailureDetail, FrameFailureHandler, FrameFailureKind, PanicText, SegmentPhase,
};
use crate::testing::ScriptedSink;

#[derive(Clone)]
struct ErrorFieldLayer {
    values: Arc<Mutex<Vec<String>>>,
}

struct ErrorFieldVisitor<'a> {
    values: &'a Arc<Mutex<Vec<String>>>,
}

impl Visit for ErrorFieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "error" {
            self.values
                .lock()
                .expect("trace field mutex")
                .push(format!("{value:?}"));
        }
    }
}

impl<S> Layer<S> for ErrorFieldLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        event.record(&mut ErrorFieldVisitor {
            values: &self.values,
        });
    }
}

fn capture_pipeline_failure(detail: FrameFailureDetail) -> (Vec<String>, bool) {
    const SENTINEL: &str = "private-pipeline-report-sentinel";

    let realm = UiRealm::for_test();
    realm.set_frame_failure_detail(detail);
    let handler_saw_typed_error = Arc::new(AtomicBool::new(false));
    let handler_saw_typed_error_for_callback = Arc::clone(&handler_saw_typed_error);
    realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        let FrameFailureKind::Pipeline {
            error: flui_rendering::RenderError::SemanticsError { message },
        } = &report.kind
        else {
            panic!("handler must receive the typed semantics error");
        };
        assert_eq!(message.as_ref(), SENTINEL);
        handler_saw_typed_error_for_callback.store(true, Ordering::Relaxed);
    })));

    let values = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Registry::default().with(ErrorFieldLayer {
        values: Arc::clone(&values),
    });
    tracing::subscriber::with_default(subscriber, || {
        realm.report_frame_failure(
            realm.presentations.primary(),
            FrameFailureKind::Pipeline {
                error: flui_rendering::RenderError::semantics(SENTINEL),
            },
        );
    });

    let values = Arc::try_unwrap(values)
        .expect("trace layer released after with_default")
        .into_inner()
        .expect("trace field mutex");
    (values, handler_saw_typed_error.load(Ordering::Relaxed))
}

fn with_quiet_panics<R>(operation: impl FnOnce() -> R) -> R {
    type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;

    struct HookRestore(Option<PanicHook>);

    impl Drop for HookRestore {
        fn drop(&mut self) {
            if let Some(hook) = self.0.take() {
                std::panic::set_hook(hook);
            }
        }
    }

    let _restore = HookRestore(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_| {}));
    operation()
}

#[test]
fn pipeline_trace_obeys_detail_policy_while_handler_keeps_the_typed_error() {
    let (redacted_fields, redacted_handler_typed) =
        capture_pipeline_failure(FrameFailureDetail::Redacted);
    assert_eq!(
        redacted_fields,
        [flui_foundation::diagnostics::REDACTED_VALUE]
    );
    assert!(redacted_handler_typed);

    let (verbatim_fields, verbatim_handler_typed) =
        capture_pipeline_failure(FrameFailureDetail::Verbatim);
    assert_eq!(verbatim_fields.len(), 1);
    assert!(
        verbatim_fields[0].contains("private-pipeline-report-sentinel"),
        "verbatim tracing must retain the pipeline error: {verbatim_fields:?}"
    );
    assert!(verbatim_handler_typed);
}

#[test]
fn explicit_segment_detail_is_profile_independent_and_classifies_before_redaction() {
    let cases = [
        (
            FrameFailureDetail::Redacted,
            "BUG: private-redacted-panic-sentinel",
            PanicText::Redacted,
            true,
        ),
        (
            FrameFailureDetail::Verbatim,
            "private-verbatim-panic-sentinel",
            PanicText::Verbatim("private-verbatim-panic-sentinel".into()),
            false,
        ),
    ];

    for (detail, payload, expected_text, expected_internal_invariant) in cases {
        let realm = UiRealm::for_test();
        realm.set_frame_failure_detail(detail);
        realm
            .attach_root_widget(&SizedBox::new(10.0, 10.0))
            .expect("attaches");
        let reports = Arc::new(Mutex::new(Vec::new()));
        let reports_for_handler = Arc::clone(&reports);
        realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            let FrameFailureKind::SegmentPanic {
                message,
                phase,
                internal_invariant,
                ..
            } = &report.kind
            else {
                panic!("segment probe must report SegmentPanic");
            };
            reports_for_handler.lock().expect("report mutex").push((
                message.clone(),
                *phase,
                *internal_invariant,
            ));
        })));
        realm.presentations.primary().set_segment_probe(
            SegmentPhase::Build,
            Some(Box::new(move || panic!("{payload}"))),
        );
        realm.request_redraw();

        let mut backend = ScriptedSink::always_presents();
        with_quiet_panics(|| realm.render_frame(&mut backend));

        assert_eq!(
            *reports.lock().expect("report mutex"),
            [(
                expected_text,
                SegmentPhase::Build,
                expected_internal_invariant,
            )]
        );
    }
}
