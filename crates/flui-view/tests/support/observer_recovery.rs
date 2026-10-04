//! Public observer callbacks with abort-capable destruction stay in children.
use flui_foundation::observe::{ElementMounted, ElementRebuilt, TreeObserver};
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_view::{BuildOwner, ElementTree, RebuildReason};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("opaque observer obligation dropped");
    }
}
struct FailingObserver {
    aggregate_payload: bool,
    fail_detach: bool,
    payload_drops: Arc<AtomicUsize>,
    _captures: Option<(Bomb, Bomb)>,
    calls: Arc<AtomicUsize>,
    detached: Arc<AtomicUsize>,
}
impl FailingObserver {
    fn fail(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.aggregate_payload {
            std::panic::panic_any((
                Bomb(Arc::clone(&self.payload_drops)),
                Bomb(Arc::clone(&self.payload_drops)),
            ));
        }
        panic!("observer callback failed");
    }
}
impl TreeObserver for FailingObserver {
    fn element_mounted(&self, _: &ElementMounted) {
        if !self.fail_detach {
            self.fail();
        }
    }
    fn detached(&self) {
        self.detached.fetch_add(1, Ordering::SeqCst);
        if self.fail_detach {
            self.fail();
        }
    }
}
#[derive(Default)]
struct HealthyObserver(AtomicUsize);
impl TreeObserver for HealthyObserver {
    fn element_rebuilt(&self, _: &ElementRebuilt) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct FailedTelemetry {
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl<S: tracing::Subscriber> Layer<S> for FailedTelemetry {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR
            && event.metadata().target().starts_with("flui_view::owner")
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::panic::panic_any((Bomb(Arc::clone(&self.drops)), Bomb(Arc::clone(&self.drops))));
        }
    }
}

fn run_child(kind: &str) {
    let detach = kind.starts_with("detach") || kind.starts_with("replace");
    let replace = kind.starts_with("replace");
    let payload = kind.contains("payload") || kind.contains("all");
    let captures = kind.contains("capture") || kind.contains("all");
    let telemetry = kind.contains("telemetry") || kind.contains("all");
    let payload_drops = Arc::new(AtomicUsize::new(0));
    let capture_drops = Arc::new(AtomicUsize::new(0));
    let telemetry_drops = Arc::new(AtomicUsize::new(0));
    let telemetry_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let detached = Arc::new(AtomicUsize::new(0));
    let mut owner = BuildOwner::new();
    owner.set_tree_observer(Arc::new(FailingObserver {
        aggregate_payload: payload,
        fail_detach: detach,
        payload_drops: Arc::clone(&payload_drops),
        _captures: captures.then(|| {
            (
                Bomb(Arc::clone(&capture_drops)),
                Bomb(Arc::clone(&capture_drops)),
            )
        }),
        calls: Arc::clone(&calls),
        detached: Arc::clone(&detached),
    }));
    let mut tree = ElementTree::new();
    let healthy = Arc::new(HealthyObserver::default());
    let subscriber = tracing_subscriber::registry().with(telemetry.then(|| FailedTelemetry {
        calls: Arc::clone(&telemetry_calls),
        drops: Arc::clone(&telemetry_drops),
    }));
    let root = tracing::subscriber::with_default(subscriber, || {
        let root = tree.mount_root_with_pipeline_owner(
            &super::PlainLeaf,
            Some(PipelineCell::new(PipelineOwner::new(
                flui_rendering::TextContextHandle::standalone(),
            ))),
            &mut owner.element_owner_mut(),
        );
        if detach {
            if replace {
                owner.set_tree_observer(healthy.clone());
            } else {
                owner.clear_tree_observer();
            }
        }
        root
    });
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the failing callback actually ran"
    );
    assert_eq!(
        detached.load(Ordering::SeqCst),
        usize::from(detach),
        "emission failure must not invoke detached"
    );
    assert_eq!(payload_drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        capture_drops.load(Ordering::SeqCst),
        0,
        "last failed observer envelope is retained"
    );
    assert_eq!(telemetry_drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        telemetry_calls.load(Ordering::SeqCst),
        usize::from(telemetry),
        "failure reporting really reached the hostile subscriber"
    );
    assert!(
        tree.get(root).is_some(),
        "mount completes after observer containment"
    );
    if replace {
        assert!(owner.tree_observer().is_some());
    } else {
        assert!(
            owner.tree_observer().is_none(),
            "failed observer is disarmed"
        );
        owner.set_tree_observer(healthy.clone());
    }
    tree.mark_needs_build(root);
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let built = healthy.0.load(Ordering::SeqCst);
    assert!(built > 0, "a new observer sees the next real build");
    tree.mark_needs_build(root);
    owner.schedule_build_for(root, 0, RebuildReason::StateChange);
    owner.build_scope(&mut tree);
    assert_eq!(
        healthy.0.load(Ordering::SeqCst),
        built + 1,
        "later frames still emit normally"
    );
    owner.clear_tree_observer();
    drop(owner);
    assert_eq!(capture_drops.load(Ordering::SeqCst), 0);
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
        .env("FLUI_OBSERVER_RECOVERY_CHILD", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("observer child");
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().expect("status").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill blocked child");
            let output = child.wait_with_output().expect("reap child");
            panic!("observer recovery blocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("test result: ok. 1 passed; 0 failed;"),
        "observer recovery failed: {output:?}"
    );
}

pub(crate) fn dispatch_child(kind: &str) {
    assert!(matches!(
        kind,
        "emit_payload"
            | "emit_capture"
            | "emit_payload_capture"
            | "emit_telemetry"
            | "emit_all"
            | "detach_payload"
            | "detach_capture"
            | "detach_all"
            | "replace_all"
    ));
    run_child(kind);
}
pub(crate) fn emission_retains_aggregate_payload() {
    child("emit_payload");
}
pub(crate) fn emission_retains_failed_capture_envelope() {
    child("emit_capture");
}
pub(crate) fn emission_retains_competing_payload_and_captures() {
    child("emit_payload_capture");
}
pub(crate) fn emission_contains_reporting_failure() {
    child("emit_telemetry");
}
pub(crate) fn emission_contains_all_opaque_obligations() {
    child("emit_all");
}
pub(crate) fn detach_retains_aggregate_payload() {
    child("detach_payload");
}
pub(crate) fn detach_retains_failed_capture_envelope() {
    child("detach_capture");
}
pub(crate) fn detach_contains_all_opaque_obligations() {
    child("detach_all");
}
pub(crate) fn replacement_preserves_its_new_observer_after_detach_failure() {
    child("replace_all");
}
