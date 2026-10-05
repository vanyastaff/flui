//! Real dense child recovery retains opaque failure ownership before cleanup.
use super::dense_reconcile_containment::{DenseHealthyLeaf, DenseRow};
use flui_rendering::{
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};
use flui_view::{
    BuildOwner, ElementTree, ErrorView, LifecycleHook, RebuildReason, RenderView, View, ViewExt,
};
use std::{
    any::TypeId,
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("opaque child payload destroyed");
    }
}
fn fail(drops: &Arc<AtomicUsize>) -> ! {
    std::panic::panic_any((Bomb(Arc::clone(drops)), Bomb(Arc::clone(drops))));
}

#[derive(Clone, Copy)]
enum Hook {
    Create,
    RenderMount,
    Update,
}
#[derive(Clone)]
struct Subject {
    hook: Hook,
    armed: Rc<Cell<bool>>,
    drops: Arc<AtomicUsize>,
}
impl RenderView for Subject {
    type Protocol = BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;
    fn create_render_object(&self, _: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        if matches!(self.hook, Hook::RenderMount) && self.armed.get() {
            fail(&self.drops);
        }
        flui_objects::RenderSizedBox::shrink()
    }
    fn update_render_object(
        &self,
        _: &flui_view::RenderObjectContext<'_>,
        _: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        if matches!(self.hook, Hook::Update) && self.armed.get() {
            fail(&self.drops);
        }
        flui_rendering::RenderUpdateImpact::NONE
    }
}
impl View for Subject {
    fn create_element(&self) -> flui_view::element::ElementKind {
        if matches!(self.hook, Hook::Create) && self.armed.get() {
            fail(&self.drops);
        }
        flui_view::element::ElementKind::render_variable(self)
    }
}
fn failing_factory(_: &flui_view::FrameworkError) -> Box<dyn View> {
    panic!("child recovery factory failed")
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
            fail(&self.drops);
        }
    }
}

pub(crate) fn dispatch_child(kind: &str) {
    let hook = if kind.starts_with("update") {
        Hook::Update
    } else if kind.starts_with("render_mount") {
        Hook::RenderMount
    } else {
        Hook::Create
    };
    let factory = kind.ends_with("factory");
    let telemetry = kind.ends_with("report");
    flui_view::clear_error_view_builder();
    let armed = Rc::new(Cell::new(!matches!(hook, Hook::Update)));
    let original_drops = Arc::new(AtomicUsize::new(0));
    let report_drops = Arc::new(AtomicUsize::new(0));
    let report_calls = Arc::new(AtomicUsize::new(0));
    let subject = Subject {
        hook,
        armed: Rc::clone(&armed),
        drops: Arc::clone(&original_drops),
    };
    let row = DenseRow {
        children: vec![
            DenseHealthyLeaf { marker: 1 }.boxed(),
            subject.boxed(),
            DenseHealthyLeaf { marker: 2 }.boxed(),
        ],
    };
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let parent = tree.mount_root_with_pipeline_owner(
        &row,
        Some(PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ))),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent, 0, RebuildReason::InitialMount);
    let old_failed = if matches!(hook, Hook::Update) {
        owner.build_scope(&mut tree);
        let old = tree.get(parent).expect("parent").child_ids()[1];
        armed.set(true);
        tree.update(parent, &row, &mut owner.element_owner_mut());
        owner.schedule_build_for(parent, 0, RebuildReason::ParentUpdate);
        Some(old)
    } else {
        None
    };
    if factory {
        flui_view::set_error_view_builder(failing_factory);
    }
    let subscriber = tracing_subscriber::registry().with(telemetry.then(|| FailedTelemetry {
        calls: Arc::clone(&report_calls),
        drops: Arc::clone(&report_drops),
    }));
    let result = tracing::subscriber::with_default(subscriber, || {
        catch_unwind(AssertUnwindSafe(|| owner.build_scope(&mut tree)))
    });
    flui_view::clear_error_view_builder();
    assert_eq!(original_drops.load(Ordering::SeqCst), 0);
    assert_eq!(report_drops.load(Ordering::SeqCst), 0);
    if factory {
        let payload = result.expect_err("factory failure remains authoritative");
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some("child recovery factory failed")
        );
        assert!(owner.take_recovered_panics().is_empty());
    } else {
        assert!(result.is_ok(), "child recovery must complete");
        let children = tree.get(parent).expect("parent survives").child_ids();
        assert_eq!(children.len(), 3);
        let substitute = children[1];
        assert_eq!(
            tree.get(substitute)
                .expect("real substitute")
                .element()
                .view_type_id(),
            TypeId::of::<ErrorView>()
        );
        for &sibling in &[children[0], children[2]] {
            assert_eq!(
                tree.get(sibling)
                    .expect("healthy sibling")
                    .element()
                    .view_type_id(),
                TypeId::of::<DenseHealthyLeaf>()
            );
        }
        let records = owner.take_recovered_panics();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].hook,
            if matches!(hook, Hook::Update) {
                LifecycleHook::Update
            } else {
                LifecycleHook::Mount
            }
        );
        assert_eq!(records[0].view_type_id, TypeId::of::<Subject>());
        assert_eq!(records[0].payload_text, None);
        assert!(
            matches!(records[0].at, flui_view::RecoveredAt::Substituted { substitute: id, parent: p, slot: 1, .. } if id == substitute && p == parent)
        );
        if let Some(old) = old_failed {
            assert!(!tree.contains(old));
        }
        assert_eq!(report_calls.load(Ordering::SeqCst), usize::from(telemetry));
    }
    // Retry through the actual parent producer, replacing any recovery child.
    armed.set(false);
    tree.update(parent, &row, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);
    owner.finalize_tree(&mut tree);
    let children = tree.get(parent).expect("retry parent").child_ids();
    assert_eq!(children.len(), 3);
    assert_eq!(
        tree.get(children[1])
            .expect("healthy retry child")
            .element()
            .view_type_id(),
        TypeId::of::<Subject>()
    );
    assert!(owner.take_recovered_panics().is_empty());
    assert_eq!(original_drops.load(Ordering::SeqCst), 0);
}

pub(super) fn run_child(variable: &str, kind: &str) {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "lifecycle_panic_containment_matrix",
            "--nocapture",
        ])
        .env(variable, kind)
        .env("RUST_BACKTRACE", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("child recovery process");
    let mut stdout = child.stdout.take().expect("child stdout");
    let mut stderr = child.stderr.take().expect("child stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).expect("drain child stdout");
        bytes
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).expect("drain child stderr");
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut timed_out = false;
    while child.try_wait().expect("child status").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill blocked child");
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = std::process::Output {
        status: child.wait().expect("reap child"),
        stdout: stdout_reader.join().expect("stdout reader"),
        stderr: stderr_reader.join().expect("stderr reader"),
    };
    assert!(!timed_out, "child recovery blocked: {output:?}");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("test result: ok. 1 passed; 0 failed;"),
        "child recovery failed: {output:?}"
    );
}
fn run(kind: &str) {
    run_child("FLUI_CHILD_PAYLOAD_RECOVERY_CHILD", kind);
}
pub(crate) fn create_payload_is_retained_before_child_recovery() {
    run("create");
}
pub(crate) fn mounted_payload_is_retained_before_child_cleanup() {
    run("render_mount");
}
pub(crate) fn update_payload_is_retained_before_child_cleanup() {
    run("update");
}
pub(crate) fn create_payload_cannot_compete_with_factory_failure() {
    run("create_factory");
}
pub(crate) fn mounted_payload_cannot_compete_with_factory_failure() {
    run("render_mount_factory");
}
pub(crate) fn update_payload_cannot_compete_with_factory_failure() {
    run("update_factory");
}
pub(crate) fn create_and_reporting_payloads_are_retained_independently() {
    run("create_report");
}
pub(crate) fn mounted_and_reporting_payloads_are_retained_independently() {
    run("render_mount_report");
}
pub(crate) fn update_and_reporting_payloads_are_retained_independently() {
    run("update_report");
}
