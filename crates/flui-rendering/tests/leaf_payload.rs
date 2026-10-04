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
#[derive(Debug)]
struct BoxParent(Source);
impl Diagnosticable for BoxParent {}
impl RenderBox for BoxParent {
    type Arity = flui_foundation::Single;
    type ParentData = BoxParentData;
    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> Size {
        throw_source_failure(&self.0);
        ctx.layout_child(0, *ctx.constraints())
    }
}

#[derive(Debug)]
struct SliverParent(Source);
impl Diagnosticable for SliverParent {}
impl flui_rendering::traits::RenderSliver for SliverParent {
    type Arity = flui_foundation::Single;
    type ParentData = flui_rendering::parent_data::SliverParentData;
    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::SliverLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::constraints::SliverGeometry {
        throw_source_failure(&self.0);
        let size = ctx.layout_box_child(0, BoxConstraints::tight(Size::new(10.0, 20.0)));
        flui_rendering::constraints::SliverGeometry::new(size.height, size.height, 0.0)
    }
}
fn throw_source_failure(source: &Source) {
    match source.failure {
        1 => std::panic::panic_any(Bomb(Arc::clone(&source.drops))),
        2 => std::panic::panic_any(CompetingBombs {
            _first: Bomb(Arc::clone(&source.drops)),
            _second: Bomb(Arc::clone(&source.drops)),
        }),
        3 => panic!("ordinary source failure"),
        _ => (),
    }
}

#[derive(Debug)]
struct SliverHost(std::rc::Rc<std::cell::Cell<flui_rendering::constraints::SliverGeometry>>);
impl Diagnosticable for SliverHost {}
impl RenderBox for SliverHost {
    type Arity = flui_foundation::Single;
    type ParentData = BoxParentData;
    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> Size {
        self.0
            .set(ctx.layout_sliver_child(0, crate::common::vertical_constraints(0.0)));
        ctx.constraints().biggest()
    }
}

fn nonleaf_failure_then_replacement(sliver: bool, failure: u8) {
    use flui_rendering::constraints::SliverGeometry;
    let drops = Arc::new(AtomicUsize::new(0));
    let captured = std::rc::Rc::new(std::cell::Cell::new(SliverGeometry::ZERO));
    let mut pipeline = crate::common::fresh_layout_pipeline();
    let constraints = BoxConstraints::tight(Size::new(10.0, 20.0));
    let install = |pipeline: &mut flui_rendering::pipeline::PipelineOwner<
        flui_rendering::pipeline::phase::Layout,
    >,
                   failure| {
        let source = Source {
            failure,
            drops: Arc::clone(&drops),
        };
        let root = if sliver {
            pipeline
                .render_tree_mut()
                .insert_box(Box::new(SliverHost(std::rc::Rc::clone(&captured))))
        } else {
            pipeline
                .render_tree_mut()
                .insert_box(Box::new(BoxParent(source)))
        };
        let parent = if sliver {
            pipeline
                .render_tree_mut()
                .insert_sliver_child(
                    root,
                    Box::new(SliverParent(Source {
                        failure,
                        drops: Arc::clone(&drops),
                    })),
                )
                .expect("sliver parent")
        } else {
            root
        };
        pipeline
            .render_tree_mut()
            .insert_box_child(
                parent,
                Box::new(Source {
                    failure: 0,
                    drops: Arc::clone(&drops),
                }),
            )
            .expect("attached leaf establishes nonleaf layout");
        root
    };
    let root = install(&mut pipeline, failure);
    let result = pipeline.layout_dirty_root(root, constraints);
    if sliver {
        result.expect("host completes with failed child's stand-in");
        assert_eq!(
            captured.get(),
            SliverGeometry::ZERO,
            "failed sliver publishes no partial geometry"
        );
        assert!(
            pipeline
                .render_tree()
                .get(root)
                .expect("host")
                .geometry_degraded(),
            "host must report failed descendant geometry"
        );
    } else {
        assert!(
            matches!(
                result,
                Err(RenderError::Poisoned {
                    phase: PoisonPhase::Layout,
                    ..
                })
            ),
            "original nonleaf layout failure remains authoritative: {result:?}"
        );
    }
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "reporting must retain opaque destruction"
    );
    pipeline.render_tree_mut().remove_recursive(root);
    let replacement = install(&mut pipeline, 0);
    assert_eq!(
        pipeline
            .layout_dirty_root(replacement, constraints)
            .expect("replacement layout"),
        Size::new(10.0, 20.0)
    );
    if sliver {
        assert_eq!(
            captured.get().paint_extent,
            20.0,
            "replacement sliver supplies actual geometry"
        );
    }
}

struct PanickingReporter(Arc<AtomicUsize>, Arc<AtomicUsize>);
impl tracing::Subscriber for PanickingReporter {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::ERROR && metadata.fields().field("panic_msg").is_some()
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::panic::panic_any(CompetingBombs {
            _first: Bomb(Arc::clone(&self.1)),
            _second: Bomb(Arc::clone(&self.1)),
        });
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
fn reporting_competition(protocol: u8, failure: u8) {
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(
        PanickingReporter(Arc::clone(&calls), Arc::clone(&drops)),
        || match protocol {
            0 => failure_then_replacement(failure),
            1 => nonleaf_failure_then_replacement(false, failure),
            _ => nonleaf_failure_then_replacement(true, failure),
        },
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "original failure reporter must actually run once"
    );
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "secondary opaque payload remains retained"
    );
}
fn box_ordinary_recovers() {
    nonleaf_failure_then_replacement(false, 3);
}
fn box_hostile_recovers() {
    nonleaf_failure_then_replacement(false, 1);
}
fn box_aggregate_recovers() {
    nonleaf_failure_then_replacement(false, 2);
}
fn sliver_ordinary_recovers() {
    nonleaf_failure_then_replacement(true, 3);
}
fn sliver_hostile_recovers() {
    nonleaf_failure_then_replacement(true, 1);
}
fn sliver_aggregate_recovers() {
    nonleaf_failure_then_replacement(true, 2);
}
fn leaf_reporting_recovers() {
    reporting_competition(0, 3);
}
fn leaf_competing_reporting_recovers() {
    reporting_competition(0, 2);
}
fn box_reporting_recovers() {
    reporting_competition(1, 3);
}
fn box_competing_reporting_recovers() {
    reporting_competition(1, 2);
}
fn sliver_reporting_recovers() {
    reporting_competition(2, 3);
}
fn sliver_competing_reporting_recovers() {
    reporting_competition(2, 2);
}
#[test]
fn layout_reporting_retains_opaque_payloads() {
    let cases: &[(&str, fn())] = &[
        ("ordinary payload", ordinary_payload_recovers),
        ("hostile payload", hostile_payload_recovers),
        ("competing fields", competing_payload_fields_recovers),
        ("box ordinary", box_ordinary_recovers),
        ("box hostile", box_hostile_recovers),
        ("box aggregate", box_aggregate_recovers),
        ("sliver ordinary", sliver_ordinary_recovers),
        ("sliver hostile", sliver_hostile_recovers),
        ("sliver aggregate", sliver_aggregate_recovers),
        ("leaf reporting", leaf_reporting_recovers),
        (
            "leaf competing reporting",
            leaf_competing_reporting_recovers,
        ),
        ("box reporting", box_reporting_recovers),
        ("box competing reporting", box_competing_reporting_recovers),
        ("sliver reporting", sliver_reporting_recovers),
        (
            "sliver competing reporting",
            sliver_competing_reporting_recovers,
        ),
    ];
    const SELECTED: &str = "FLUI_LAYOUT_PAYLOAD_CASE";
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
                    "leaf_payload::layout_reporting_retains_opaque_payloads",
                    "--nocapture",
                ])
                .env(SELECTED, name)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("layout payload child");
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
