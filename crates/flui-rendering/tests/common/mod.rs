//! Scaffolding shared by the `rendering_it` integration-test modules.
//!
//! Every root test file compiles as a module of the single `rendering_it`
//! binary (see `main.rs`), so plumbing that used to be copied per file —
//! boxed-object aliases, layout drivers, geometry readers — lives here
//! once. Mock render objects (hosts, probes, stub slivers) stay local to
//! the tests that exercise them; only protocol-neutral plumbing belongs
//! in this module.

use flui_foundation::RenderId;
use flui_foundation::geometry::Size;
use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    pipeline::{PipelineOwner, phase::Layout},
    protocol::{BoxProtocol, SliverProtocol},
    testing::{inspect, sliver as sliver_presets},
    traits::RenderObject,
};

/// A boxed Box-protocol render object, as stored in the pipeline tree.
pub type BoxedRenderObject = Box<dyn RenderObject<BoxProtocol>>;

/// A boxed Sliver-protocol render object, as stored in the pipeline tree.
pub type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

/// A fresh pipeline owner already advanced into the Layout phase. Tests
/// build the tree via `render_tree_mut` (phase-agnostic accessor) before
/// driving layout.
pub fn fresh_layout_pipeline() -> PipelineOwner<Layout> {
    PipelineOwner::new(flui_rendering::TextContextHandle::standalone()).into_layout()
}

/// Installs `root` with the given root constraints, runs layout, and
/// returns the owner in the Layout phase for inspection.
pub fn laid_out_with(
    mut owner: PipelineOwner,
    root: RenderId,
    constraints: BoxConstraints,
) -> PipelineOwner<Layout> {
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(constraints));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");
    owner
}

/// [`laid_out_with`] under a tight 300×100 root — the shared viewport of
/// the sliver-family harness tests.
pub fn laid_out_tight_300x100(owner: PipelineOwner, root: RenderId) -> PipelineOwner<Layout> {
    laid_out_with(owner, root, BoxConstraints::tight(Size::new(300.0, 100.0)))
}

/// [`laid_out_with`] under a tight 100×100 root.
pub fn laid_out_tight_100x100(owner: PipelineOwner, root: RenderId) -> PipelineOwner<Layout> {
    laid_out_with(owner, root, BoxConstraints::tight(Size::new(100.0, 100.0)))
}

/// [`laid_out_with`] under a loose 0–200 × 0–200 root.
pub fn laid_out_loose_200x200(owner: PipelineOwner, root: RenderId) -> PipelineOwner<Layout> {
    laid_out_with(owner, root, BoxConstraints::new(0.0, 200.0, 0.0, 200.0))
}

/// Reads the committed [`SliverGeometry`] for `id`, panicking when layout
/// has not produced one.
pub fn sliver_geometry(owner: &PipelineOwner<Layout>, id: RenderId) -> SliverGeometry {
    inspect::sliver_geometry(owner, id).expect("sliver geometry is committed")
}

/// Exit status of an isolated case that ran to completion. libtest exits 0
/// when its filter matches no test, so success must be a status only the
/// case itself can produce.
const ISOLATED_CASE_PASSED: i32 = 42;

/// Ends an isolated child process after its case completed.
pub fn isolated_case_passed() -> ! {
    std::process::exit(ISOLATED_CASE_PASSED)
}

/// Runs the test named `test` in a child process with `selector=case` in its
/// environment, bounded by a timeout. The child must finish through
/// [`isolated_case_passed`]; an abort, a failed assertion, a stall or a
/// filter that matched nothing is an error carrying the child's stderr.
pub fn run_isolated(test: &str, selector: &str, case: &str) -> Result<(), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", test, "--include-ignored", "--nocapture"])
        .env(selector, case)
        .env("RUST_BACKTRACE", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn isolated case");
    let mut stderr = child.stderr.take().expect("stderr pipe");
    let stderr = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).expect("stderr read");
        text
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().expect("child status") {
            break (status, false);
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill stalled child");
            break (child.wait().expect("reap stalled child"), true);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stderr = stderr.join().expect("stderr reader");
    if !timed_out && status.code() == Some(ISOLATED_CASE_PASSED) {
        Ok(())
    } else {
        let stall = if timed_out { " (timed out)" } else { "" };
        Err(format!("{case}: {status}{stall}\n{stderr}"))
    }
}

/// Vertical sliver constraints for the shared 300-wide × 100-tall test
/// viewport: 100 px of paint room, a 120 px cache window starting 20 px
/// before the leading edge.
pub fn vertical_constraints(scroll_offset: f64) -> SliverConstraints {
    sliver_presets::vertical()
        .scroll_offset(scroll_offset)
        .remaining_paint_extent(100.0)
        .cross_axis_extent(300.0)
        .viewport_main_axis_extent(100.0)
        .remaining_cache_extent(120.0)
        .cache_origin(-20.0)
        .build()
}
