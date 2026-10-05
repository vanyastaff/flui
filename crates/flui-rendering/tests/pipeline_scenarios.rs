//! Render-object foundation under realistic multi-frame scenarios.
//!
//! Each scenario drives the REAL frame pipeline (layout → compositing →
//! paint) the bindings use in production, then asserts on observable
//! outputs: committed offsets, layer-tree structure, picture bounds, hit
//! paths, dirty-queue hygiene, and — via the `Diagnosticable`-backed
//! diagnostics — the render objects' self-described configuration.
//!
//! These scenarios are expressed with the `flui_rendering::testing` harness
//! (`RenderTester` / `FrameRun` / `Probe`): declarative tree specs, symmetric
//! `run_frame`, `update` + `pump` for multi-frame mutation, and structured
//! property queries. The advanced layer-internal checks (transform matrices,
//! clip shapes, offset layers) still walk the produced `LayerTree` directly.
//!
//! Scenarios:
//! 1. deep nesting — offsets accumulate through a 50-deep padding chain and
//!    a single merged picture comes out;
//! 2. mixed tree — flex + padding + transform + clip in one frame;
//! 3. invalidation round-trips — paint-only then layout-changing frames;
//! 4. idle stability — frames after a clean one produce NO output;
//! 5. removal churn — remove + reinsert under one parent;
//! 6. repaint-boundary subtree — the boundary's OffsetLayer split survives
//!    re-frames;
//! 7. edge cases — zero sizes and empty containers;
//! 8. churn stress — 20 remove+reinsert cycles.

use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{
    constraints::BoxConstraints,
    testing::{Probe, RenderTester, box_node},
};

/// Callback capture retirement can deadlock against the retained node sender,
/// so every setter/failure row runs in a bounded public consumer process.
#[test]
fn pipeline_callback_replacement_reentry() {
    use flui_rendering::pipeline::{PipelineOwner, RenderInvalidationHandle};
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const CHILD: &str = "FLUI_PIPELINE_REPLACEMENT_CHILD";
    if let Ok(case) = std::env::var(CHILD) {
        struct RetiredWake {
            case: String,
            handle: RenderInvalidationHandle,
            drops: Arc<AtomicUsize>,
            fail: bool,
        }
        impl Drop for RetiredWake {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::Relaxed);
                println!("pipeline replacement retirement entered: {}", self.case);
                self.handle
                    .mark_needs_paint()
                    .expect("retiring capture can enqueue and wake");
                assert!(!self.fail, "first displaced callback retirement");
            }
        }
        fn install(
            owner: &mut PipelineOwner,
            event: &str,
            callback: impl Fn() + Send + Sync + 'static,
        ) {
            match event {
                "visual" => owner.set_on_need_visual_update(callback),
                "created" => owner.set_on_semantics_owner_created(callback),
                "disposed" => owner.set_on_semantics_owner_disposed(callback),
                _ => panic!("unknown callback event"),
            }
        }
        let (event, failure) = case.split_once('-').expect("event/failure case");
        assert!(matches!(failure, "healthy" | "drop"));
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let id = owner.set_root_render_object(Box::new(RenderColoredBox::red(10.0, 10.0)));
        let handle = owner
            .render_invalidation_handle(id)
            .expect("attached public node sender");
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_capture = Arc::clone(&wake_count);
        owner.set_on_need_visual_update(move || {
            wake_capture.fetch_add(1, Ordering::Relaxed);
        });
        if event == "disposed" {
            owner.set_semantics_enabled(true);
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let retired = RetiredWake {
            case: case.clone(),
            handle: handle.clone(),
            drops: Arc::clone(&drops),
            fail: failure == "drop",
        };
        install(&mut owner, event, move || {
            let _ = &retired;
        });
        wake_count.store(0, Ordering::Relaxed);
        let latest = Arc::new(AtomicUsize::new(0));
        let latest_capture = Arc::clone(&latest);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            install(&mut owner, event, move || {
                latest_capture.fetch_add(1, Ordering::Relaxed);
            });
        }));
        if failure == "drop" {
            let payload = result.expect_err("displaced capture failure propagates");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("first displaced callback retirement")
            );
        } else {
            result.expect("healthy displaced capture retires");
        }
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        if event == "visual" {
            assert_eq!(
                latest.load(Ordering::Relaxed),
                1,
                "reentrant wake observes committed replacement"
            );
        } else {
            assert_eq!(
                wake_count.load(Ordering::Relaxed),
                1,
                "retirement wake remains deliverable"
            );
        }
        handle
            .mark_needs_paint()
            .expect("next independent handle wake");
        match event {
            "visual" => assert_eq!(latest.load(Ordering::Relaxed), 2),
            "created" => {
                owner.set_semantics_enabled(true);
                assert_eq!(latest.load(Ordering::Relaxed), 1);
            }
            "disposed" => {
                owner.set_semantics_enabled(false);
                assert_eq!(latest.load(Ordering::Relaxed), 1);
            }
            _ => unreachable!(),
        }
        let final_count = Arc::new(AtomicUsize::new(0));
        let final_capture = Arc::clone(&final_count);
        install(&mut owner, event, move || {
            final_capture.fetch_add(1, Ordering::Relaxed);
        });
        match event {
            "visual" => owner.request_visual_update(),
            "created" => {
                owner.set_semantics_enabled(false);
                owner.set_semantics_enabled(true);
            }
            "disposed" => {
                owner.set_semantics_enabled(true);
                owner.set_semantics_enabled(false);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            final_count.load(Ordering::Relaxed),
            1,
            "subsequent replacement stays authoritative"
        );
        println!("pipeline replacement completed: {case}");
        return;
    }
    let mut failures = Vec::new();
    let selected = std::env::var("FLUI_PIPELINE_REPLACEMENT_CONTROL").ok();
    if let Some(selected) = &selected {
        assert!(
            matches!(
                selected.as_str(),
                "visual-healthy"
                    | "visual-drop"
                    | "created-healthy"
                    | "created-drop"
                    | "disposed-healthy"
                    | "disposed-drop"
            ),
            "unknown replacement control row"
        );
    }
    for case in [
        "visual-healthy",
        "visual-drop",
        "created-healthy",
        "created-drop",
        "disposed-healthy",
        "disposed-drop",
    ] {
        if selected.as_deref().is_some_and(|selected| selected != case) {
            continue;
        }
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "pipeline_scenarios::pipeline_callback_replacement_reentry",
                "--nocapture",
            ])
            .env(CHILD, case)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("pipeline replacement child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("stdout read");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("stderr read");
            text
        });
        let started = Instant::now();
        let mut timed_out = false;
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                timed_out = true;
                child.kill().expect("kill stalled child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success()
            || !stdout.contains("1 passed; 0 failed")
            || !stdout.contains(&format!("pipeline replacement completed: {case}"))
        {
            failures.push(format!(
                "{case}: timed_out={timed_out}, {status}\n{stdout}\n{stderr}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Loose `0..=hi x 0..=hi` constraints (children settle at natural size).
fn loose(width: f64, height: f64) -> BoxConstraints {
    BoxConstraints::new(0.0, width, 0.0, height)
}

// ============================================================================
// 1. Deep nesting: 50 paddings of 1px each around a 10×10 box
// ============================================================================

// ============================================================================
// 2. Mixed tree in one frame
// ============================================================================

// ============================================================================
// 3. Invalidation round-trips across three frames
// ============================================================================

// ============================================================================
// 4. Idle stability: clean frames produce no output
// ============================================================================

// ============================================================================
// 5. Removal churn under one parent
// ============================================================================

// ============================================================================
// 6. Repaint-boundary split survives re-frames
// ============================================================================

// ============================================================================
// 7. Edge cases: zero sizes and empty containers
// ============================================================================

// ============================================================================
// 8. Churn stress: 20 remove+reinsert cycles with frames between
// ============================================================================

pub(crate) fn repeated_churn_cycles_stay_clean_and_generations_protect_every_round() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("initial")),
    )
    .with_constraints(loose(200.0, 200.0))
    .run_frame();
    let pad = run.root();
    let mut current = run.id("initial");
    assert!(run.painted(), "initial frame paints");

    let mut stale_ids = Vec::new();
    for round in 0..20u32 {
        assert_eq!(
            run.owner_mut().remove_render_object(current),
            1,
            "round {round}"
        );
        stale_ids.push(current);
        let side = 10.0 + round as f64;
        current = run
            .owner_mut()
            .insert_child_render_object(pad, Box::new(RenderColoredBox::blue(side, side)))
            .expect("reinserted child");
        run.owner_mut().mark_needs_layout(pad);
        let report = run.pump();
        assert!(report.painted, "round {round}: churn frame must paint");
    }

    assert!(run.is_clean(), "no residue after 20 churn rounds");

    // EVERY historical id must stay dead — slot reuse never resurrects an
    // old handle, no matter how many generations passed.
    for (i, stale) in stale_ids.iter().enumerate() {
        assert!(
            run.owner().render_tree().get(*stale).is_none(),
            "stale id from round {i} must not resolve",
        );
    }
    assert!(run.owner().render_tree().get(current).is_some());
    assert_eq!(run.hit(20.0, 20.0).first().copied(), Some(current));
}

// ====================================================================
// Deferred mutations integration tests
// ====================================================================
