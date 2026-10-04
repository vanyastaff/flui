//! Opaque layout failure payloads do not execute destruction while reporting.

use flui_foundation::{Diagnosticable, Leaf, geometry::Size};
use flui_rendering::{
    TextContextHandle,
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    error::{PoisonPhase, RenderError},
    parent_data::BoxParentData,
    protocol::BoxProtocol,
    storage::RenderEntry,
    traits::RenderBox,
};
use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("opaque payload destructor");
    }
}
struct CompetingBombs {
    _first: Bomb,
    _second: Bomb,
}

#[derive(Debug)]
struct Source {
    failure: u8,
    drops: Arc<AtomicUsize>,
}
impl Diagnosticable for Source {}
impl RenderBox for Source {
    type Arity = Leaf;
    type ParentData = BoxParentData;
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        match self.failure {
            1 => std::panic::panic_any(Bomb(Arc::clone(&self.drops))),
            2 => std::panic::panic_any(CompetingBombs {
                _first: Bomb(Arc::clone(&self.drops)),
                _second: Bomb(Arc::clone(&self.drops)),
            }),
            3 => panic!("ordinary source failure"),
            _ => ctx.constraints().constrain(Size::new(10.0, 20.0)),
        }
    }
    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        false
    }
}
fn failure_then_replacement(failure: u8) {
    let drops = Arc::new(AtomicUsize::new(0));
    let text = TextContextHandle::standalone();
    let constraints = BoxConstraints::tight(Size::new(10.0, 20.0));
    let mut entry = RenderEntry::<BoxProtocol>::new(Box::new(Source {
        failure,
        drops: Arc::clone(&drops),
    }));
    let error = entry
        .layout_leaf_only(constraints, text.source())
        .expect_err("source must refuse layout");
    assert!(
        matches!(
            error,
            RenderError::Poisoned {
                phase: PoisonPhase::Layout,
                ..
            }
        ),
        "original layout failure must remain authoritative: {error:?}"
    );
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "reporting must not destroy opaque payloads"
    );
    entry = RenderEntry::<BoxProtocol>::new(Box::new(Source { failure: 0, drops }));
    assert_eq!(
        entry
            .layout_leaf_only(constraints, text.source())
            .expect("replacement layout"),
        Size::new(10.0, 20.0)
    );
}
fn ordinary_payload_recovers() {
    failure_then_replacement(3);
}
fn hostile_payload_recovers() {
    failure_then_replacement(1);
}
fn competing_payload_fields_recovers() {
    failure_then_replacement(2);
}
#[test]
fn leaf_layout_reporting_retains_opaque_payloads() {
    let cases: &[(&str, fn())] = &[
        ("ordinary payload", ordinary_payload_recovers),
        ("hostile payload", hostile_payload_recovers),
        ("competing fields", competing_payload_fields_recovers),
    ];
    const SELECTED: &str = "FLUI_LEAF_PAYLOAD_CASE";
    if let Ok(selected) = std::env::var(SELECTED) {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case")
            .1();
        return;
    }
    let mut failures = Vec::new();
    for (name, _) in cases {
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "leaf_payload::leaf_layout_reporting_retains_opaque_payloads",
                    "--nocapture",
                ])
                .env(SELECTED, name)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("leaf payload child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut output = String::new();
            stdout.read_to_string(&mut output).expect("stdout read");
            output
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut output = String::new();
            stderr.read_to_string(&mut output).expect("stderr read");
            output
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill stalled child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
