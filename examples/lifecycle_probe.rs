//! `lifecycle_probe` — a self-driving window-lifecycle probe for
//! `docs/BETA.md`'s lifecycle rows on macOS (`just macos-lifecycle`).
//!
//! Runs a Material tree through the ordinary `flui::app::Application` path
//! — the same runner, renderer and realm a generated application gets — and
//! then drives the window through the transitions a user performs on it,
//! from a driver thread, through AppKit on the main queue, with no operator
//! input and no synthetic OS events:
//!
//! | phase | what the driver does | what is observed |
//! | --- | --- | --- |
//! | `first_frame` | waits for the first frame to run | seconds from the window factory to that frame |
//! | `visible` | nothing; the tree animates | frames run in 2 s (the baseline) |
//! | `minimized` | `-[NSWindow miniaturize:]`, settles 0.5 s | frames run in 3 s |
//! | `restored` | `-[NSWindow deminiaturize:]`, settles 0.5 s | frames run in 2 s |
//! | `hidden` | `-[NSApplication hide:]`, settles 0.5 s | frames run in 3 s |
//! | `unhidden` | `-[NSApplication unhide:]`, settles 0.5 s | frames run in 2 s |
//! | `resized` | `-[NSWindow setFrame:display:]` to +200×+100 | the constraints the root `LayoutBuilder` last saw |
//!
//! "Frames run" counts a self-re-arming post-frame callback (see
//! `examples/workload_probe.rs`, "Idle-phase frame counting", for why that
//! observer contributes no frames of its own). A free-running
//! [`AnimationController`] keeps demanding frames throughout, so a phase
//! whose count is near zero shows the runner refusing that demand — which
//! is the property being verified for a minimized or hidden window: a
//! window nobody can see must not cost a frame loop — and a phase whose
//! count is back near the baseline shows the refusal ending when the
//! window comes back.
//!
//! # Budgets, declared before the first run
//!
//! - `first_frame`: a frame runs within 15 s (a hang guard; the time is the
//!   number of interest).
//! - `visible`, `restored`, `unhidden`: at least [`RESUMED_MIN_FRAMES`]
//!   frames in their 2 s window — a resumed window animates again.
//! - `minimized`, `hidden`: at most [`SUPPRESSED_MAX_FRAMES`] frames in
//!   their 3 s window — a handful is allowed for the transition's own
//!   redraws, a display period's worth per second is not.
//! - `resized`: the root's constraints equal the window's new content size
//!   (±1 logical pixel) within 1 s.
//!
//! The process exits 0 with `LIFECYCLE_PROBE_RESULT=PASS` on stdout when
//! every budget holds and 1 with `=FAIL` otherwise; every phase prints one
//! JSON line first, so a failure carries its numbers. A frame that runs but
//! presents nothing (an occluded surface) still counts here — this probe
//! measures whether the loop runs, not whether the compositor shows it.
//!
//! # macOS only
//!
//! The transitions are AppKit's. On any other host the binary prints a
//! skip line and exits 0; the `just` recipe does the same without building.

#[cfg(target_os = "macos")]
mod probe {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use flui::animation::{Animation, AnimationController, Vsync, VsyncRegistration};
    use flui::app::{AppConfig, AppHandle, Application, StartupWindow};
    use flui::foundation::Listenable;
    use flui::material::{Scaffold, Theme, ThemeData};
    use flui::prelude::*;
    use flui::view::PostFrameHandle;
    use flui::widgets::{LayoutBuilder, VsyncScope};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use parking_lot::Mutex;

    /// A resumed (or never-suppressed) 2 s window must run at least this
    /// many frames: a quarter of a 60 Hz panel's worth, so a hiccup on
    /// restore is tolerated and a stalled loop is not.
    pub const RESUMED_MIN_FRAMES: u64 = 30;
    /// A minimized or hidden 3 s window may run at most this many frames:
    /// the transition's own redraws, not a running loop.
    pub const SUPPRESSED_MAX_FRAMES: u64 = 5;
    /// The first frame must run within this long of the driver starting
    /// (which is the main-window factory running): a hang guard, not a
    /// performance budget — the measured time is reported.
    const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(15);
    /// The resize must reach layout within this long.
    const RESIZE_SETTLE: Duration = Duration::from_secs(1);
    const SETTLE: Duration = Duration::from_millis(500);
    const RESUMED_WINDOW: Duration = Duration::from_secs(2);
    const SUPPRESSED_WINDOW: Duration = Duration::from_secs(3);
    /// The size the driver resizes to, as a delta on the initial frame.
    const RESIZE_DELTA: (f64, f64) = (200.0, 100.0);

    const INITIAL_SIZE: (u32, u32) = (640, 480);

    /// Everything the driver thread reads: frames run so far, and the
    /// constraints the root `LayoutBuilder` saw last.
    struct Witness {
        frames: AtomicU64,
        /// `(max_width, max_height)` in logical pixels, ×1000 for atomic
        /// storage.
        constraints_milli: Mutex<Option<(u64, u64)>>,
    }

    fn schedule_frame_observer(post_frame: PostFrameHandle, witness: Arc<Witness>) {
        let next = post_frame.clone();
        post_frame.schedule(move |_timing| {
            witness.frames.fetch_add(1, Ordering::SeqCst);
            schedule_frame_observer(next, witness);
        });
    }

    #[derive(Clone, StatefulView)]
    struct ProbeRoot {
        witness: Arc<Witness>,
    }

    struct ProbeRootState {
        witness: Arc<Witness>,
        controller: AnimationController,
        registration: Option<(Vsync, VsyncRegistration)>,
    }

    impl StatefulView for ProbeRoot {
        type State = ProbeRootState;

        fn create_state(&self) -> Self::State {
            ProbeRootState {
                witness: Arc::clone(&self.witness),
                controller: AnimationController::with_detached_ticker(Duration::from_millis(1_000)),
                registration: None,
            }
        }
    }

    impl ViewState<ProbeRoot> for ProbeRootState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            if let Some(handle) = ctx.post_frame_handle() {
                schedule_frame_observer(handle, Arc::clone(&self.witness));
            }
            // The controller's value drives a rebuild every tick, so the
            // tree is genuinely animating, not merely ticking.
            let rebuild = ctx.rebuild_handle();
            self.controller.add_listener(Arc::new(move || {
                rebuild.schedule(flui::foundation::RebuildReason::StateChange);
            }));
            if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
                let registration = vsync.register(self.controller.clone());
                self.registration = Some((vsync, registration));
            }
            self.controller
                .repeat(true)
                .expect("a freshly created controller accepts repeat()");
        }

        fn dispose(&mut self) {
            if let Some((vsync, registration)) = self.registration.take() {
                vsync.unregister(registration);
            }
        }

        fn build(&self, _view: &ProbeRoot, _ctx: &dyn BuildContext) -> impl IntoView {
            let witness = Arc::clone(&self.witness);
            let value = self.controller.value();
            Theme::new(
                ThemeData::light(),
                Scaffold::new().body(LayoutBuilder::new(move |_ctx, constraints| {
                    *witness.constraints_milli.lock() = Some((
                        (f64::from(constraints.max_width.0) * 1000.0).round() as u64,
                        (f64::from(constraints.max_height.0) * 1000.0).round() as u64,
                    ));
                    // A bar that sweeps with the controller: the frame has
                    // something to draw differently each tick.
                    let width = 40.0 + 200.0 * value;
                    Align::new(Alignment::CENTER_LEFT).child(
                        Container::new()
                            .width(width)
                            .height(24.0)
                            .color(Color::rgb(98, 0, 238)),
                    )
                })),
            )
        }
    }

    /// Run `body` on the AppKit main queue and wait for it. The probe's
    /// driver thread has no AppKit affinity; every window call goes
    /// through here.
    fn on_main<R: Send + 'static>(body: impl FnOnce(MainThreadMarker) -> R + Send + 'static) -> R {
        let (tx, rx) = std::sync::mpsc::channel();
        dispatch2::DispatchQueue::main().exec_async(move || {
            let mtm = MainThreadMarker::new().expect("the main dispatch queue is the main thread");
            let _ = tx.send(body(mtm));
        });
        rx.recv().expect("the main-queue body always answers")
    }

    fn main_window(mtm: MainThreadMarker) -> objc2::rc::Retained<NSWindow> {
        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        windows
            .iter()
            .next()
            .expect("the application opened its main window before the driver started")
    }

    struct PhaseResult {
        name: &'static str,
        passed: bool,
    }

    fn count_frames(witness: &Witness, window: Duration) -> u64 {
        let before = witness.frames.load(Ordering::SeqCst);
        std::thread::sleep(window);
        witness.frames.load(Ordering::SeqCst) - before
    }

    fn report_frames(
        name: &'static str,
        frames: u64,
        window: Duration,
        budget: &str,
        passed: bool,
    ) {
        println!(
            "{{\"phase\":\"{name}\",\"frames\":{frames},\"window_s\":{:.1},\"budget\":\"{budget}\",\"pass\":{passed}}}",
            window.as_secs_f64()
        );
    }

    /// Count frames over `window` and record the phase against its bound.
    fn frames_phase(
        witness: &Witness,
        results: &mut Vec<PhaseResult>,
        name: &'static str,
        window: Duration,
        min: Option<u64>,
        max: Option<u64>,
    ) {
        let frames = count_frames(witness, window);
        let (passed, budget) = match (min, max) {
            (Some(min), _) => (frames >= min, format!(">={min}")),
            (_, Some(max)) => (frames <= max, format!("<={max}")),
            _ => unreachable!("every phase declares a bound"),
        };
        report_frames(name, frames, window, &budget, passed);
        results.push(PhaseResult { name, passed });
    }

    fn drive(witness: Arc<Witness>, app: AppHandle) -> Vec<PhaseResult> {
        let mut results = Vec::new();

        // The baseline starts at the first frame, not at launch: on a cold
        // launch the GPU stack behind the window takes seconds to build
        // (2.81 s measured in docs/BETA.md), and the frame loop does not
        // run until it exists. The wait itself is reported as the time to
        // the first frame.
        let started = std::time::Instant::now();
        let first_frame_deadline = started + FIRST_FRAME_TIMEOUT;
        while witness.frames.load(Ordering::SeqCst) == 0
            && std::time::Instant::now() < first_frame_deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let first_frame_after = started.elapsed();
        let first_frame_seen = witness.frames.load(Ordering::SeqCst) > 0;
        println!(
            "{{\"phase\":\"first_frame\",\"after_s\":{:.3},\"budget\":\"<={:.0}s\",\"pass\":{first_frame_seen}}}",
            first_frame_after.as_secs_f64(),
            FIRST_FRAME_TIMEOUT.as_secs_f64()
        );
        results.push(PhaseResult {
            name: "first_frame",
            passed: first_frame_seen,
        });
        std::thread::sleep(SETTLE);
        frames_phase(
            &witness,
            &mut results,
            "visible",
            RESUMED_WINDOW,
            Some(RESUMED_MIN_FRAMES),
            None,
        );

        on_main(|mtm| main_window(mtm).miniaturize(None));
        std::thread::sleep(SETTLE);
        let minimized = on_main(|mtm| main_window(mtm).isMiniaturized());
        println!("{{\"phase\":\"minimize\",\"is_miniaturized\":{minimized}}}");
        frames_phase(
            &witness,
            &mut results,
            "minimized",
            SUPPRESSED_WINDOW,
            None,
            Some(SUPPRESSED_MAX_FRAMES),
        );

        on_main(|mtm| main_window(mtm).deminiaturize(None));
        std::thread::sleep(SETTLE);
        frames_phase(
            &witness,
            &mut results,
            "restored",
            RESUMED_WINDOW,
            Some(RESUMED_MIN_FRAMES),
            None,
        );

        on_main(|mtm| NSApplication::sharedApplication(mtm).hide(None));
        std::thread::sleep(SETTLE);
        let hidden = on_main(|mtm| NSApplication::sharedApplication(mtm).isHidden());
        println!("{{\"phase\":\"hide\",\"is_hidden\":{hidden}}}");
        frames_phase(
            &witness,
            &mut results,
            "hidden",
            SUPPRESSED_WINDOW,
            None,
            Some(SUPPRESSED_MAX_FRAMES),
        );

        on_main(|mtm| NSApplication::sharedApplication(mtm).unhide(None));
        std::thread::sleep(SETTLE);
        frames_phase(
            &witness,
            &mut results,
            "unhidden",
            RESUMED_WINDOW,
            Some(RESUMED_MIN_FRAMES),
            None,
        );

        // Resize: grow the frame, then compare the content size AppKit
        // reports with the constraints layout saw.
        let expected = on_main(|mtm| {
            let window = main_window(mtm);
            let frame = window.frame();
            let new_frame = NSRect::new(
                NSPoint::new(frame.origin.x, frame.origin.y - RESIZE_DELTA.1),
                NSSize::new(
                    frame.size.width + RESIZE_DELTA.0,
                    frame.size.height + RESIZE_DELTA.1,
                ),
            );
            window.setFrame_display(new_frame, true);
            let content = window.contentRectForFrameRect(window.frame());
            (content.size.width, content.size.height)
        });
        let deadline = std::time::Instant::now() + RESIZE_SETTLE;
        let mut seen = None;
        let mut matched = false;
        while std::time::Instant::now() < deadline {
            if let Some((w, h)) = *witness.constraints_milli.lock() {
                let (w, h) = (w as f64 / 1000.0, h as f64 / 1000.0);
                seen = Some((w, h));
                if (w - expected.0).abs() <= 1.0 && (h - expected.1).abs() <= 1.0 {
                    matched = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let (seen_w, seen_h) = seen.unwrap_or((f64::NAN, f64::NAN));
        println!(
            "{{\"phase\":\"resized\",\"expected\":[{:.1},{:.1}],\"layout_saw\":[{seen_w:.1},{seen_h:.1}],\"pass\":{matched}}}",
            expected.0, expected.1
        );
        results.push(PhaseResult {
            name: "resized",
            passed: matched,
        });

        let _ = app.request_quit();
        results
    }

    pub fn run() -> i32 {
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()))
            .with_writer(std::io::stderr)
            .init();

        let witness = Arc::new(Witness {
            frames: AtomicU64::new(0),
            constraints_milli: Mutex::new(None),
        });
        let results: Arc<Mutex<Option<Vec<PhaseResult>>>> = Arc::new(Mutex::new(None));

        let witness_for_factory = Arc::clone(&witness);
        let results_for_driver = Arc::clone(&results);
        let driver_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = Application::new(move |handle: &AppHandle| {
            // The factory runs once per main window; the driver starts with
            // the first one and never twice.
            if !driver_started.swap(true, Ordering::SeqCst) {
                let witness = Arc::clone(&witness_for_factory);
                let handle = handle.clone();
                let results = Arc::clone(&results_for_driver);
                std::thread::spawn(move || {
                    let outcome = drive(witness, handle);
                    *results.lock() = Some(outcome);
                });
            }
            ProbeRoot {
                witness: Arc::clone(&witness_for_factory),
            }
        })
        .with_config(
            AppConfig::new()
                .with_title("FLUI Lifecycle Probe")
                .with_size(INITIAL_SIZE.0, INITIAL_SIZE.1),
        )
        .with_startup_window(StartupWindow::Open)
        .run();

        if let Err(error) = result {
            eprintln!("lifecycle_probe: application run failed: {error}");
            return 1;
        }
        let Some(results) = results.lock().take() else {
            println!(
                "LIFECYCLE_PROBE_RESULT=FAIL (the application exited before the driver finished)"
            );
            return 1;
        };
        let failed: Vec<&str> = results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.name)
            .collect();
        if failed.is_empty() {
            println!("LIFECYCLE_PROBE_RESULT=PASS ({} phases)", results.len());
            0
        } else {
            println!(
                "LIFECYCLE_PROBE_RESULT=FAIL (phases over budget: {})",
                failed.join(", ")
            );
            1
        }
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    std::process::exit(probe::run());
    #[cfg(not(target_os = "macos"))]
    println!("lifecycle_probe: SKIP — the transitions it drives are AppKit's; run it on macOS");
}
