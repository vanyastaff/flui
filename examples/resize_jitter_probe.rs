//! Resize-jitter probe — can a live resize burst make the windowed renderer
//! present a frame that was rendered for the *previous* window size?
//!
//! This probe was built to be the executable half of the rationale pinned on
//! `desired_maximum_frame_latency: 1` in
//! [`flui_engine::wgpu::Renderer::derive_surface_config`]. That literal is held
//! at its tightest possible value on the stated grounds that a latency of 2
//! "lets the present queue hold frames rendered for an older size, which the
//! compositor then stretches to the current window → visible resize jitter",
//! and its own comment says re-coupling the two swapchain widths "needs its own
//! resize-jitter regression test".
//!
//! **It is not that test, and the measurement is why.** Driving a real
//! 40-resize burst while rendering continuously into the Metal swapchain
//! produces **zero** size-mismatched acquires — at latency 1 *and* at latency 2,
//! and again with the surface deliberately held three frames behind the window.
//! Four runs, four zeros. The reason is structural rather than incidental:
//! `Renderer::render_scene` acquires and presents inside a single call, and
//! `Renderer::resize` reconfigures before that call, so no drawable is ever
//! alive across a `Surface::configure` — and on Metal the layer allocates
//! drawables at its *current* `drawableSize`, so a reconfigure cannot be
//! followed by an older-size drawable. The hazard the comment names is not
//! reachable through this backend's frame pipeline. What this probe therefore
//! pins is the weaker-but-real invariant below; it does **not** discriminate
//! the literal, and the comment on that literal has been corrected to say so.
//!
//! **What it does pin.** The acquired swapchain texture must always match the
//! configured surface size. That is a real bug class — a driver or a backend
//! change that started pooling drawables across a reconfigure would hand back a
//! stale-size texture, and the frame would present stretched — and this is the
//! only executable check of it on this backend. It is kept for that, with the
//! honest caveat above rather than a claim of coverage it does not provide.
//!
//! **What it measures.** `Renderer::warn_on_size_mismatch` already carries the
//! observable: it fires when the acquired swapchain texture's dimensions differ
//! from the configured surface size at the moment of acquisition — precisely the
//! frame a compositor would have to scale. It needs no new instrumentation. The
//! probe counts that event and **fails the run on a non-zero count**; a probe
//! that asserted only "frames kept arriving" would pass with the hazard fully
//! present.
//!
//! **Why it needs a real Mac and a bundled `.app`.** It opens a *visible*
//! AppKit window and renders into its real swapchain — the mismatch is a
//! property of the platform's surface implementation (Metal here), so it cannot
//! be reproduced against a mock surface without assuming the answer. Same two
//! floors as the other AppKit probes: AppKit wants a main thread, and libtest
//! runs `#[test]` bodies on workers; an unbundled `NSWindow` throws
//! `_CFBundleGetValueForInfoKey`, a foreign NSException Rust cannot catch. Run
//! it via `just macos-resize-jitter`. On every other target the binary is a
//! compile-time no-op `main`.
//!
//! **Reporting.** `RESIZE_JITTER_PROBE_RESULT=PASS` requires three things
//! together: enough frames rendered, enough resizes actually applied, and a
//! stale-size count of exactly zero. Each is reported separately so a failure
//! says which one broke — "the window never resized" and "the swapchain went
//! stale" are different findings and must not collapse into one red marker.

#[cfg(target_os = "macos")]
mod appkit_resize_jitter_probe {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, Weak};
    use std::time::{Duration, Instant};

    use flui_engine::PresentDisposition;
    use flui_engine::wgpu::Renderer;
    use flui_layer::{LayerTree, Scene};
    use flui_platform::Platform;
    use flui_platform::traits::PlatformWindow;
    use flui_types::geometry::{Size, px};
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    /// The event target `Renderer::warn_on_size_mismatch` emits on. Counting by
    /// target, not by matching the message prose: the message is for a human
    /// reading a log, and a test that greps it breaks the moment someone
    /// rewords a comment-adjacent string.
    const STALE_TARGET: &str = "flui.gpu.resize_transient";

    /// The sizes the burst cycles through. Deliberately all different in both
    /// axes: a resize that changes only one dimension still exercises the
    /// reconfiguration, but it would not catch an axis-confused comparison.
    const SIZES: [(u32, u32); 4] = [(520, 340), (760, 420), (600, 300), (840, 480)];

    /// How long the frame pump is given to come up before the burst starts.
    /// AppKit must be running display passes first, or the "frames" count
    /// measures the warm-up rather than the burst.
    const WARMUP: Duration = Duration::from_secs(2);
    /// Gap between scripted resizes. Faster than the 10 ms panel period is
    /// pointless (a resize cannot be observed by a frame that never ran) and
    /// slower than a few periods would leave the queue drained between them,
    /// which is exactly the state that cannot show the hazard. This lands a
    /// resize roughly every 4 frames at 100 Hz.
    const RESIZE_INTERVAL: Duration = Duration::from_millis(40);
    /// How many resizes the burst performs.
    const RESIZES: usize = 40;
    /// Settle time after the last resize, so any frame still queued when the
    /// burst ended is rendered and counted before the verdict is taken.
    const SETTLE: Duration = Duration::from_millis(750);

    /// Frames the run must contain to be a measurement at all.
    const MIN_FRAMES: u64 = 100;
    /// Resizes that must actually have reached the renderer. Below this the
    /// burst never happened and a zero stale count would be vacuous — the
    /// vacuous-pass guard, and the reason this is asserted separately from the
    /// frame floor.
    const MIN_RESIZES: u64 = 20;

    /// The deadline for the pump to produce its first frame.
    const START_DEADLINE: Duration = Duration::from_secs(8);

    /// Frames the renderer's surface is held behind the window's size.
    ///
    /// This is the whole point of the probe and the reason a same-frame version
    /// of it proves nothing: if the renderer reconfigures in the same callback
    /// that acquires, `Surface::configure` always precedes
    /// `get_current_texture()`, every acquired texture is by construction the
    /// configured size, and the invariant holds trivially at any latency. The
    /// real application does not work that way — a window resize arrives
    /// asynchronously on the platform's own callback and the renderer follows it
    /// on a later frame — and the window between "the window's size changed" and
    /// "this surface was reconfigured" is exactly where `get_current_texture`
    /// answers `Suboptimal` with a texture at the *previous* size. A few frames
    /// of lag keeps the probe inside that window for a large fraction of the
    /// run.
    const FOLLOW_LAG_FRAMES: usize = 3;

    /// Counts events on [`STALE_TARGET`]. A layer rather than a log scrape
    /// because the verdict below is taken *in-process*: the probe must be able
    /// to fail on its own, so that `just macos-resize-jitter` asserts one
    /// marker instead of re-deriving the answer from captured text.
    struct StaleSizeLayer(Arc<AtomicUsize>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for StaleSizeLayer {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if event.metadata().target() == STALE_TARGET {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// What the frame callback and the resize thread share.
    ///
    /// One mutex over one small struct rather than an atomic per field: the
    /// frame callback reads the scripted index and writes three counters, and
    /// the reporter reads them all as one consistent snapshot.
    #[derive(Default)]
    struct ProbeState {
        /// Index into [`SIZES`] the burst thread last selected. The frame
        /// callback is what applies it to the renderer, so the two never race
        /// over the GPU object.
        size_index: usize,
        /// The size the renderer's surface was last configured for. `None`
        /// until the first frame, which is what makes that frame configure.
        applied: Option<(u32, u32)>,
        /// Size indices the window has taken, newest last, not yet handed to
        /// the renderer. Drained at [`FOLLOW_LAG_FRAMES`] to keep the surface
        /// deliberately behind the window.
        follow: VecDeque<usize>,
        frames: u64,
        presented: u64,
        resizes_applied: u64,
        burst_done: bool,
    }

    /// Report a fatal setup failure and exit non-zero.
    fn fatal(message: String) -> ! {
        tracing::error!("RESIZE_JITTER_PROBE_FAILURE={message}");
        tracing::error!("RESIZE_JITTER_PROBE_RESULT=FAIL");
        std::process::exit(1);
    }

    pub(crate) fn run() {
        let stale = Arc::new(AtomicUsize::new(0));

        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer())
            .with(StaleSizeLayer(Arc::clone(&stale)))
            .init();

        // The production launch path — `MacOSPlatform::new` → `Platform::run` —
        // rather than a hand-rolled loop: `run` activates the app before the
        // event loop starts, and an inactive app gets no display passes at all.
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
            .run(Box::new(move |owner| {
                setup_and_run(owner, stale);
                Ok(())
            }))
            .expect("Platform::run must not return an error");
    }

    /// The probe, run from `on_finish_launching` — after the app is activated
    /// and immediately before `NSApplication::run`.
    fn setup_and_run(owner: flui_platform::OwnerPlatform, stale: Arc<AtomicUsize>) {
        let (first_w, first_h) = SIZES[0];
        let window = match owner.open_window(flui_platform::WindowOptions {
            title: "FLUI resize-jitter probe".to_string(),
            size: Size::new(px(first_w as f32), px(first_h as f32)),
            // Resizable, unlike the other AppKit probes: this one exists to
            // change the window's size, so a non-resizable window would make
            // the platform refuse the very operation under test.
            resizable: true,
            visible: true,
            decorated: true,
            min_size: None,
            max_size: None,
        }) {
            Ok(pending) => match pending.try_ready() {
                Ok(window) => window,
                Err(error) => fatal(format!("the window was not ready: {error:?}")),
            },
            Err(error) => fatal(format!("open_window was refused: {error:?}")),
        };

        // `Renderer::new` takes ownership of an owned handle source; going
        // through `Arc<dyn PlatformWindow>` is the caller shape its own doc
        // names as the common one.
        let mut renderer = match pollster::block_on(Renderer::new(Arc::clone(&window))) {
            Ok(renderer) => renderer,
            Err(error) => fatal(format!("Renderer::new failed: {error:?}")),
        };

        let state = Arc::new(Mutex::new(ProbeState::default()));

        // The frame body: apply whatever size the burst thread last chose,
        // force damage, render. The size is applied HERE and not on the burst
        // thread so the `Renderer` is only ever touched from the main thread —
        // it is `Send` but not `Sync`, and the frame callback is where the
        // engine's own frames drive it.
        let frame_state = Arc::clone(&state);
        let window_ref: Weak<dyn PlatformWindow> = Arc::downgrade(&window);
        window.on_request_frame(Box::new(move || {
            if let Ok(mut state) = frame_state.lock() {
                // Hold the surface deliberately behind the window: push the
                // window's current size and apply only what has aged out of the
                // queue, so `applied` trails the real window by
                // `FOLLOW_LAG_FRAMES` for the whole burst.
                let window_index = state.size_index;
                state.follow.push_back(window_index);
                while state.follow.len() > FOLLOW_LAG_FRAMES {
                    let index = state.follow.pop_front().unwrap_or(0);
                    let target = SIZES[index];
                    if state.applied != Some(target) {
                        renderer.resize(target.0, target.1);
                        state.applied = Some(target);
                        state.resizes_applied += 1;
                    }
                }
                // The scene is drawn at whatever the surface is actually
                // configured for — not at the window's size, which is the
                // mismatch this probe is here to catch.
                let target = state.applied.unwrap_or(SIZES[0]);
                // The engine resets damage at the end of every rendered frame,
                // so a probe that wants a frame per request must ask for one —
                // this is the same call `flui-app`'s direct path makes.
                renderer.mark_full_repaint();

                let tree = LayerTree::new();
                let root = tree.root();
                let frame = state.frames;
                let scene = Scene::new(
                    Size::new(px(target.0 as f32), px(target.1 as f32)),
                    tree,
                    root,
                    frame,
                );
                if let Ok(PresentDisposition::Presented) = renderer.render_scene(&scene) {
                    state.presented += 1;
                }
                state.frames += 1;
            }
            // The re-arm, from inside the frame — the same shape the frame-pump
            // probe pins, and the call the display-pass deferral makes land.
            if let Some(window) = window_ref.upgrade() {
                window.request_redraw();
            }
        }));

        // The burst thread: a real window resize on a schedule, plus the index
        // the frame callback picks up. The window resize is what makes this a
        // *window* resize rather than a bare surface reconfiguration — the
        // scripted index alone would only ever move the renderer's own size.
        let burst_state = Arc::clone(&state);
        let burst_window = Arc::downgrade(&window);
        std::thread::spawn(move || {
            std::thread::sleep(WARMUP);
            for step in 0..RESIZES {
                let index = step % SIZES.len();
                let (w, h) = SIZES[index];
                if let Ok(mut state) = burst_state.lock() {
                    state.size_index = index;
                }
                if let Some(window) = burst_window.upgrade() {
                    window.resize(Size::new(px(w as f32), px(h as f32)));
                }
                std::thread::sleep(RESIZE_INTERVAL);
            }
            if let Ok(mut state) = burst_state.lock() {
                state.burst_done = true;
            }
        });

        // The reporter: the AppKit run loop never returns, so the verdict is
        // taken on its own thread and exits the process.
        std::thread::spawn(move || {
            let started = Instant::now();
            while state.lock().map_or(0, |s| s.frames) == 0 {
                if started.elapsed() >= START_DEADLINE {
                    tracing::error!(
                        "RESIZE_JITTER_PROBE_FAILURE=no frame within {START_DEADLINE:?}. The \
                         window may be occluded or the app not active — this is NOT the \
                         resize transient this probe measures."
                    );
                    tracing::error!("RESIZE_JITTER_PROBE_RESULT=FAIL");
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_millis(25));
            }

            let burst_deadline = Instant::now() + WARMUP + START_DEADLINE;
            loop {
                if state.lock().is_ok_and(|s| s.burst_done) {
                    break;
                }
                if Instant::now() >= burst_deadline {
                    tracing::warn!("the resize burst did not finish before its deadline");
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            std::thread::sleep(SETTLE);

            let snapshot = match state.lock() {
                Ok(state) => (
                    state.frames,
                    state.presented,
                    state.resizes_applied,
                    state.size_index,
                ),
                Err(_) => (0, 0, 0, 0),
            };
            let (frames, presented, resizes, last_index) = snapshot;
            let stale_count = stale.load(Ordering::SeqCst);

            tracing::info!(
                frames,
                presented,
                resizes,
                last_size = ?SIZES[last_index],
                stale_size_frames = stale_count,
                "resize-jitter probe measured"
            );
            tracing::info!("RESIZE_JITTER_PROBE_STALE={stale_count}");

            if frames < MIN_FRAMES {
                tracing::error!(
                    "RESIZE_JITTER_PROBE_FAILURE=too few frames: {frames} rendered, floor \
                     {MIN_FRAMES}. The pump did not keep up with the burst, so the burst was \
                     not observed — this run measured nothing either way."
                );
                tracing::error!("RESIZE_JITTER_PROBE_RESULT=FAIL");
                std::process::exit(1);
            }
            if resizes < MIN_RESIZES {
                tracing::error!(
                    "RESIZE_JITTER_PROBE_FAILURE=too few resizes: {resizes} reached the \
                     renderer, floor {MIN_RESIZES}. A zero stale count over a burst that never \
                     happened would be vacuous, which is why this is asserted separately."
                );
                tracing::error!("RESIZE_JITTER_PROBE_RESULT=FAIL");
                std::process::exit(1);
            }
            if stale_count > 0 {
                tracing::error!(
                    "RESIZE_JITTER_PROBE_FAILURE={stale_count} frames were presented from a \
                     swapchain texture whose size differed from the configured surface size \
                     over {resizes} resizes. Those are the frames a compositor stretches — \
                     resize jitter, present and counted."
                );
                tracing::error!("RESIZE_JITTER_PROBE_RESULT=FAIL");
                std::process::exit(1);
            }

            tracing::info!("RESIZE_JITTER_PROBE_RESULT=PASS");
            std::process::exit(0);
        });

        window.activate();
        tracing::info!("probe armed; the AppKit run loop starts next");
    }
}

#[cfg(target_os = "macos")]
fn main() {
    appkit_resize_jitter_probe::run();
}

/// Non-macOS build placeholder: this probe needs the AppKit main thread, a
/// bundle, a real display and the Metal swapchain it measures; on other targets
/// it exists only so the workspace compiles. Run it with
/// `just macos-resize-jitter` on a real Mac.
#[cfg(not(target_os = "macos"))]
fn main() {}
