//! Executable macOS frame-pump coverage: does the native AppKit backend keep
//! producing frames, or does it stop after one?
//!
//! The defect this measures is the frame source itself. `drawRect:` is the
//! backend's only frame source, and the frame it dispatches asks for its next
//! frame from exactly there (`drawRect:` → frame request → `request_redraw`) —
//! inside AppKit's display pass, the one moment a `setNeedsDisplay:` is
//! discarded. So the pump used to be circular: one frame, no wake, no frame.
//! `super::display_pass` carries the isolated measurement, and
//! `dispatch_redraw_request` is the branch that fixes it.
//!
//! This probe is the *behavioural* half of that pin, and it is the only test
//! that can observe the fix end-to-end on a real Mac: it drives the real
//! backend through the production launch path (`MacOSPlatform::new` →
//! `Platform::run` → a visible window → the real AppKit run loop), installs a
//! frame callback that re-arms exactly the way the engine's frame does (count,
//! then `request_redraw()` from inside the frame), and requires frames to keep
//! arriving after the primer that started them has stopped. A pump that
//! delivers one frame and stalls reports `FRAME_PUMP_PROBE_RESULT=FAIL`.
//!
//! Why the primer cannot explain a pass: it pokes `request_redraw()` only until
//! the FIRST frame arrives and then stops for good, so every frame counted in
//! the measurement window came from the frame's own re-arm. That is the
//! counterfactual the fix is about — with the deferral removed, the same run
//! counts zero frames in the window and fails (measured; see the run logs in
//! `.rust-studio/specs/macos-native-vsync-pacing-evidence/`).
//!
//! macOS-only by construction: besides the main-thread floor (libtest runs
//! `#[test]` bodies on a worker thread, and AppKit requires main-thread window
//! construction), unbundled NSWindow construction throws
//! `_CFBundleGetValueForInfoKey`, a foreign NSException Rust cannot catch. Run
//! it on a real Mac via `just macos-frame-pump`, which stages this example into
//! a minimal `.app` (the committed `Info.plist.frame_pump_probe` clears the
//! bundle floor), launches it with `RUST_LOG=info`, and asserts exit 0 plus the
//! PASS marker. On every other target the binary is a compile-time no-op main.

#[cfg(target_os = "macos")]
mod appkit_frame_pump_probe {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Weak};
    use std::time::{Duration, Instant};

    use flui_platform::Platform;
    use flui_types::geometry::{Size, px};
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    /// How long the primer may keep poking before the pump is declared
    /// unstarted. Generous: it has to cover app activation, the first display
    /// pass and a window that is briefly occluded while it appears.
    const START_DEADLINE: Duration = Duration::from_secs(6);
    /// How long the pump is measured once it has started.
    const MEASURE_WINDOW: Duration = Duration::from_secs(3);
    /// Frames the measurement window must contain to pass. The panel this was
    /// measured on runs at 100 Hz, so a healthy pump produces ~300; a stalled
    /// one produces 0 and a one-frame pump cannot reach 100 by any pacing at
    /// all. The floor is deliberately far below the expected rate: this probe
    /// asserts liveness, not pacing (which the `flui.gpu` present traces
    /// measure — see the spec's evidence logs).
    const MIN_FRAMES_IN_WINDOW: usize = 100;

    /// Report a fatal setup failure and exit non-zero.
    fn fatal(message: String) -> ! {
        tracing::error!("FRAME_PUMP_PROBE_FAILURE={message}");
        tracing::error!("FRAME_PUMP_PROBE_RESULT=FAIL");
        std::process::exit(1);
    }

    pub(crate) fn run() {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer())
            .init();

        // The production launch path — `MacOSPlatform::new` → `Platform::run` —
        // rather than a hand-rolled loop: `run` activates the app before it
        // starts the event loop, and an inactive app gets no display passes at
        // all, which would make this probe fail for a reason that has nothing to
        // do with the pump.
        let platform = flui_platform::MacOSPlatform::new()
            .expect("MacOSPlatform::new must succeed on the AppKit main thread");
        if platform.name() != "macOS (AppKit)" {
            fatal(format!(
                "expected platform macOS (AppKit), got {}",
                platform.name()
            ));
        }
        tracing::info!("platform: {}", platform.name());

        Box::new(platform)
            .run(Box::new(|owner| {
                setup_and_run(owner);
                Ok(())
            }))
            .expect("Platform::run must not return an error");
    }

    /// The whole probe, run from `on_finish_launching` — i.e. after the app is
    /// activated and immediately before `NSApplication::run`.
    fn setup_and_run(owner: flui_platform::OwnerPlatform) {
        // A VISIBLE window: this probe needs AppKit to run display passes, and
        // a window that is never ordered front (the close-path probe's shape)
        // gets none. Occlusion is the honest reason a run can produce no frames
        // at all, which is why the failure messages below distinguish "never
        // started" from "started and stalled".
        let window = match owner.open_window(flui_platform::WindowOptions {
            title: "FLUI frame-pump probe".to_string(),
            size: Size::new(px(480.0), px(320.0)),
            resizable: false,
            visible: true,
            decorated: true,
            min_size: None,
            max_size: None,
            ..Default::default()
        }) {
            Ok(pending) => match pending.try_ready() {
                Ok(window) => window,
                Err(error) => fatal(format!("the window was not ready: {error:?}")),
            },
            Err(error) => fatal(format!("open_window was refused: {error:?}")),
        };

        // What this window reports for its display period, from the real
        // backend on the real display — the value `flui-app`'s runner paces
        // against in place of its 60 Hz default, and the one a unit test of
        // the arithmetic cannot reach (it needs the live NSScreen → CGDisplay
        // → current-mode path). Reported as its own marker, not folded into
        // the pump result: a display that reports no rate makes this `None`
        // without saying anything about the pump, and the two claims fail
        // independently.
        if let Some(period) = window.refresh_period() {
            tracing::info!(
                period_us = period.as_micros() as u64,
                hz = period.as_secs_f64().recip(),
                "FRAME_PUMP_PROBE_REFRESH_PERIOD=reported"
            );
        } else {
            tracing::warn!(
                "FRAME_PUMP_PROBE_REFRESH_PERIOD=unreported: refresh_period() returned None, so \
                 the runner paces against its default 60 Hz period on this display"
            );
        }

        // The frame body: count, then ask for the next frame from inside the
        // frame — the engine's own shape, and the call the display pass
        // discards when it is not deferred. The window is reached through a
        // `Weak` so the callback does not keep its own window alive (the
        // callback is stored by the window's callback registry).
        let frames = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&frames);
        let window_ref: Weak<dyn flui_platform::PlatformWindow> = Arc::downgrade(&window);
        window.on_request_frame(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            if let Some(window) = window_ref.upgrade() {
                window.request_redraw();
            }
        }));

        // The primer: the only thing that starts the pump. It stops the moment
        // the first frame arrives, so it cannot manufacture the measurement.
        let primer_done = Arc::new(AtomicBool::new(false));
        let primer_flag = Arc::clone(&primer_done);
        let primer_window = Arc::downgrade(&window);
        let primer_frames = Arc::clone(&frames);
        std::thread::spawn(move || {
            // Let the run loop come up before the first poke: a cross-thread
            // `request_redraw` is a synchronous hop onto the main queue, and
            // before `NSApp::run` services it the hop has nothing to land on.
            std::thread::sleep(Duration::from_millis(500));
            let deadline = Instant::now() + START_DEADLINE;
            while Instant::now() < deadline {
                if primer_frames.load(Ordering::SeqCst) > 0 {
                    break;
                }
                if let Some(window) = primer_window.upgrade() {
                    window.request_redraw();
                } else {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            primer_flag.store(true, Ordering::SeqCst);
        });

        // The reporter: measures the pump and exits the process (the AppKit run
        // loop below never returns, so reporting cannot happen after it).
        let report_frames = Arc::clone(&frames);
        std::thread::spawn(move || {
            let start_deadline = Instant::now() + START_DEADLINE;
            while report_frames.load(Ordering::SeqCst) == 0 {
                if Instant::now() >= start_deadline {
                    tracing::error!(
                        "FRAME_PUMP_PROBE_FAILURE=pump never started: no display pass reached \
                         the view within {START_DEADLINE:?}. The window may be occluded or the \
                         app not active — this is NOT the stall this probe measures."
                    );
                    tracing::error!("FRAME_PUMP_PROBE_RESULT=FAIL");
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_millis(50));
            }

            // The primer must be done before the window is trusted: a frame
            // counted while the primer is still poking could have been its
            // doing.
            let primer_deadline = Instant::now() + START_DEADLINE;
            while !primer_done.load(Ordering::SeqCst) {
                if Instant::now() >= primer_deadline {
                    tracing::warn!("the primer did not stop within {START_DEADLINE:?}");
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }

            let first = report_frames.load(Ordering::SeqCst);
            let measured_from = Instant::now();
            std::thread::sleep(MEASURE_WINDOW);
            let second = report_frames.load(Ordering::SeqCst);
            let produced = second.saturating_sub(first);
            let seconds = measured_from.elapsed().as_secs_f64();

            tracing::info!(
                primer_frames = first,
                produced,
                seconds,
                fps = produced as f64 / seconds,
                "frame pump measured"
            );

            if produced >= MIN_FRAMES_IN_WINDOW {
                tracing::info!("FRAME_PUMP_PROBE_RESULT=PASS");
                std::process::exit(0);
            }
            tracing::error!(
                "FRAME_PUMP_PROBE_FAILURE=pump stalled: {produced} frames in {seconds:.2}s after \
                 {first} frames before the measurement began (floor {MIN_FRAMES_IN_WINDOW}). The \
                 frame that ran did not re-arm the pump — the display-pass deferral in \
                 dispatch_redraw_request is what makes that re-arm land."
            );
            tracing::error!("FRAME_PUMP_PROBE_RESULT=FAIL");
            std::process::exit(1);
        });

        // Order the window front before the loop starts. `Platform::run`
        // activates the app itself right after this callback returns; this is
        // the window half of the same requirement.
        window.activate();
        tracing::info!("probe armed; the AppKit run loop starts next");
    }
}

#[cfg(target_os = "macos")]
fn main() {
    appkit_frame_pump_probe::run();
}

/// Non-macOS build placeholder: this probe needs the AppKit main thread, a
/// bundle, and a real display to produce frames at all; on other targets it
/// exists only so the workspace compiles. Run it with `just macos-frame-pump`
/// on a real Mac.
#[cfg(not(target_os = "macos"))]
fn main() {}
