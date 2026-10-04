//! Recovery owns opaque payloads before invoking factories and diagnostics.
use flui_view::{
    BuildContext, BuildOwner, ElementTree, ErrorView, IntoView, LifecycleHook, RebuildReason,
    RecoveredAt, StatefulView, StatelessView, View, ViewExt, ViewState,
};
use std::{
    any::TypeId,
    cell::Cell,
    rc::Rc,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("opaque build payload destroyed");
    }
}

#[derive(Clone)]
struct BuildSubject {
    fail: Rc<Cell<bool>>,
    aggregate: bool,
    calls: Rc<Cell<usize>>,
    drops: Arc<AtomicUsize>,
}
impl StatelessView for BuildSubject {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        self.calls.set(self.calls.get() + 1);
        if self.fail.get() {
            if self.aggregate {
                std::panic::panic_any((
                    Bomb(Arc::clone(&self.drops)),
                    Bomb(Arc::clone(&self.drops)),
                ));
            }
            panic!("original build failure");
        }
        super::PlainLeaf
    }
}
impl View for BuildSubject {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct InitSubject {
    observed: Rc<Cell<Option<flui_foundation::ElementId>>>,
}
struct InitState {
    observed: Rc<Cell<Option<flui_foundation::ElementId>>>,
}
impl StatefulView for InitSubject {
    type State = InitState;
    fn create_state(&self) -> InitState {
        InitState {
            observed: Rc::clone(&self.observed),
        }
    }
}
impl ViewState<InitSubject> for InitState {
    fn init_state(&mut self, context: &dyn flui_view::LifecycleContext) {
        self.observed.set(Some(context.element_id()));
        panic!("original init failure");
    }
    fn build(&self, _: &InitSubject, _: &dyn BuildContext) -> impl IntoView {
        super::PlainLeaf
    }
}
impl View for InitSubject {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}
#[derive(Clone)]
struct Host {
    subject: BuildSubject,
    init: bool,
    observed: Rc<Cell<Option<flui_foundation::ElementId>>>,
}
impl StatelessView for Host {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        if self.init && self.subject.fail.get() {
            InitSubject {
                observed: Rc::clone(&self.observed),
            }
            .boxed()
        } else {
            self.subject.clone().boxed()
        }
    }
}
impl View for Host {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

struct FailedTelemetry {
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl<S: tracing::Subscriber> Layer<S> for FailedTelemetry {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR
            && event.metadata().target() == "flui_view::owner::element_owner"
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::panic::panic_any((Bomb(Arc::clone(&self.drops)), Bomb(Arc::clone(&self.drops))));
        }
    }
}

// Each row has its own process; only the configured fn-pointer factory needs a slot.
static CAPTURE_DROPS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
struct UnwindBomb(Arc<AtomicUsize>);
impl Drop for UnwindBomb {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("recovery view destroyed during a reporting unwind");
        }
    }
}
#[derive(Clone)]
struct CapturedErrorLeaf {
    _envelope: Arc<(UnwindBomb, UnwindBomb)>,
}
impl flui_view::RenderView for CapturedErrorLeaf {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;
    fn create_render_object(&self, _: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        flui_objects::RenderSizedBox::shrink()
    }
    fn update_render_object(
        &self,
        _: &flui_view::RenderObjectContext<'_>,
        _: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}
impl View for CapturedErrorLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}
fn captured_error_leaf(_: &flui_view::FrameworkError) -> Box<dyn View> {
    let drops = CAPTURE_DROPS
        .get()
        .expect("child initialized factory captures");
    Box::new(CapturedErrorLeaf {
        _envelope: Arc::new((UnwindBomb(Arc::clone(drops)), UnwindBomb(Arc::clone(drops)))),
    })
}
fn failing_factory(_: &flui_view::FrameworkError) -> Box<dyn View> {
    panic!("recovery factory failed")
}

fn run_child(kind: &str) {
    flui_view::clear_error_view_builder();
    let aggregate = kind.contains("payload") || kind == "factory";
    let telemetry = kind.contains("report") || kind == "staged";
    let captured = kind == "captured_report";
    let original_drops = Arc::new(AtomicUsize::new(0));
    let report_drops = Arc::new(AtomicUsize::new(0));
    let capture_drops = Arc::new(AtomicUsize::new(0));
    let report_calls = Arc::new(AtomicUsize::new(0));
    if captured {
        CAPTURE_DROPS
            .set(Arc::clone(&capture_drops))
            .expect("fresh child factory slot");
        flui_view::set_error_view_builder(captured_error_leaf);
    } else if kind == "factory" {
        flui_view::set_error_view_builder(failing_factory);
    }
    let fail = Rc::new(Cell::new(true));
    let calls = Rc::new(Cell::new(0));
    let host = Host {
        init: kind == "staged",
        observed: Rc::new(Cell::new(None)),
        subject: BuildSubject {
            fail: Rc::clone(&fail),
            aggregate,
            calls: Rc::clone(&calls),
            drops: Arc::clone(&original_drops),
        },
    };
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let parent = tree.mount_root_with_pipeline_owner(
        &super::Row {
            children: vec![host.clone().boxed()],
        },
        Some(flui_rendering::pipeline::PipelineCell::new(
            flui_rendering::pipeline::PipelineOwner::new(
                flui_rendering::TextContextHandle::standalone(),
            ),
        )),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent, 0, RebuildReason::InitialMount);
    let subscriber = tracing_subscriber::registry().with(telemetry.then(|| FailedTelemetry {
        calls: Arc::clone(&report_calls),
        drops: Arc::clone(&report_drops),
    }));
    let result = tracing::subscriber::with_default(subscriber, || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner.build_scope(&mut tree);
        }))
    });
    // Clear before any failing assertion can leave process-global factory obligations.
    flui_view::clear_error_view_builder();
    let root = tree
        .get(parent)
        .expect("render parent remains live")
        .child_ids()[0];
    if kind == "factory" {
        let payload = result.expect_err("configured factory failure remains authoritative");
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some("recovery factory failed")
        );
        assert!(
            owner.take_recovered_panics().is_empty(),
            "factory failure did not commit recovery"
        );
    } else {
        result.expect("build failure is recovered despite diagnostic failure");
        assert_recovery(&tree, &mut owner, root, &host, captured);
    }
    if telemetry {
        assert!(
            report_calls.load(Ordering::SeqCst) > 0,
            "real recovery event reached subscriber"
        );
    }
    assert_eq!(original_drops.load(Ordering::SeqCst), 0);
    assert_eq!(report_drops.load(Ordering::SeqCst), 0);
    assert_eq!(capture_drops.load(Ordering::SeqCst), 0);
    assert_next_builds(&mut tree, &mut owner, root, &fail, &calls);
    assert_eq!(original_drops.load(Ordering::SeqCst), 0);
    assert_eq!(report_drops.load(Ordering::SeqCst), 0);
    assert_eq!(capture_drops.load(Ordering::SeqCst), 0);
}

fn assert_next_builds(
    tree: &mut ElementTree,
    owner: &mut BuildOwner,
    root: flui_foundation::ElementId,
    fail: &Cell<bool>,
    calls: &Cell<usize>,
) {
    fail.set(false);
    let before = calls.get();
    tree.mark_needs_build(root);
    owner.schedule_build_for(root, 1, RebuildReason::StateChange);
    owner.build_scope(tree);
    assert!(calls.get() > before, "next real subject build ran");
    let subject = tree.get(root).expect("root remains live").child_ids()[0];
    let leaf = tree.get(subject).expect("healthy subject").child_ids()[0];
    assert_eq!(
        tree.get(leaf)
            .expect("healthy rendered child")
            .element()
            .view_type_id(),
        TypeId::of::<super::PlainLeaf>()
    );
    let before = calls.get();
    tree.mark_needs_build(subject);
    owner.schedule_build_for(subject, 2, RebuildReason::StateChange);
    owner.build_scope(tree);
    assert_eq!(calls.get(), before + 1);
    owner.finalize_tree(tree);
    assert!(
        owner.take_recovered_panics().is_empty(),
        "healthy retries record no extra failures"
    );
}

fn assert_recovery(
    tree: &ElementTree,
    owner: &mut BuildOwner,
    root: flui_foundation::ElementId,
    host: &Host,
    captured: bool,
) {
    let recovered = owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "original attribution recorded exactly once"
    );
    let panic = &recovered[0];
    let subject = tree.get(root).expect("root").child_ids()[0];
    let leaf = if host.init {
        subject
    } else {
        tree.get(subject).expect("subject").child_ids()[0]
    };
    assert_eq!(
        tree.get(leaf)
            .expect("actual recovery child")
            .element()
            .view_type_id(),
        if captured {
            TypeId::of::<CapturedErrorLeaf>()
        } else {
            TypeId::of::<ErrorView>()
        }
    );
    if host.init {
        assert_eq!(panic.hook, LifecycleHook::InitState);
        assert_eq!(panic.view_type_id, TypeId::of::<InitSubject>());
        assert!(
            matches!(panic.at, RecoveredAt::Element { element, parent: None, .. } if Some(element) == host.observed.get())
        );
        assert_eq!(panic.payload_text.as_deref(), Some("original init failure"));
    } else {
        assert_eq!(panic.hook, LifecycleHook::Build);
        assert_eq!(panic.view_type_id, TypeId::of::<BuildSubject>());
        assert!(
            matches!(panic.at, RecoveredAt::Element { element, parent: None, .. } if element == subject)
        );
        assert_eq!(
            panic.payload_text.as_deref(),
            if host.subject.aggregate {
                None
            } else {
                Some("original build failure")
            }
        );
    }
}

fn child(kind: &str) {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "lifecycle_panic_containment_matrix",
            "--nocapture",
        ])
        .env("FLUI_BUILD_PAYLOAD_RECOVERY_CHILD", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("recovery child");
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().expect("status").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill blocked child");
            let output = child.wait_with_output().expect("reap child");
            panic!("build recovery blocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("test result: ok. 1 passed; 0 failed;"),
        "build recovery failed: {output:?}"
    );
}
pub(crate) fn dispatch_child(kind: &str) {
    assert!(matches!(
        kind,
        "payload" | "report" | "payload_report" | "captured_report" | "factory" | "staged"
    ));
    run_child(kind);
}
pub(crate) fn aggregate_build_payload_is_retained_before_recovery() {
    child("payload");
}
pub(crate) fn recovery_reporting_preserves_original_attribution() {
    child("report");
}
pub(crate) fn original_and_reporting_payloads_do_not_compete_at_retirement() {
    child("payload_report");
}
pub(crate) fn recovery_view_survives_reporting_with_opaque_captures() {
    child("captured_report");
}
pub(crate) fn recovery_factory_failure_keeps_its_authority_after_opaque_build_failure() {
    child("factory");
}
pub(crate) fn staged_lifecycle_attribution_survives_reporting_failure() {
    child("staged");
}
