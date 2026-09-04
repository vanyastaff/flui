//! The Linux/X11 implementation — see the crate doc in `main.rs`.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as ShapeConnectionExt};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ClipOrdering, ConnectionExt, CreateWindowAux, EventMask,
    ImageFormat, Window, WindowClass,
};
use x11rb::protocol::xtest::ConnectionExt as XTestConnectionExt;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

const WINDOW_TIMEOUT: Duration = Duration::from_secs(30);
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// XTEST core-pointer event codes (X11 `MotionNotify`/`ButtonPress`/
/// `ButtonRelease` — `x11rb` exposes `fake_input` untyped).
const MOTION_NOTIFY: u8 = 6;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;
/// X11 core button number for a wheel-down tick — chosen over wheel-up so
/// the check's premise is position-independent: content is taller than the
/// viewport, so scrolling DOWN always has room, while an up-tick at an
/// already-settled-to-start position clamps to a correct no-op and fails
/// the check for the wrong reason (exactly what CI runs hit: handler traces
/// showed `delta=-53 target=0 pixels=0` — delivery fine, premise wrong).
///
/// Note: each XTEST press+release pair yields TWO `MouseWheel` events under
/// winit 0.30's X11 backend — `xinput2_button_input` runs for both
/// `XI_ButtonPress` and `XI_ButtonRelease` and its wheel arm ignores the
/// press/release state. Real hardware wheels are unaffected (they arrive as
/// XInput2 axis motion; the emulated button events carry `XIPointerEmulated`
/// and are suppressed), so this doubles the synthetic scroll distance and
/// nothing else — the check only asserts that pixels changed.
const WHEEL_DOWN: u8 = 5;

pub(crate) fn run(app_path: &str) -> Result<()> {
    let log_path =
        std::env::temp_dir().join(format!("flui-live-smoke-app-{}.log", std::process::id()));
    let log_file = std::fs::File::create(&log_path)
        .with_context(|| format!("creating {}", log_path.display()))?;

    let mut app = Command::new(app_path)
        // The app must open its window on the SAME X display this harness
        // drives. A leaked WAYLAND_DISPLAY would win backend selection and
        // put the window on the developer's real compositor instead of the
        // harness's (usually Xvfb) X server.
        .env_remove("WAYLAND_DISPLAY")
        // Diagnosable-by-default: when a check fails on a CI runner the
        // captured stderr is the only witness, and the default filter
        // logs next to nothing.
        .env(
            "RUST_LOG",
            // `flui.gpu=trace` powers the occlusion check's oracle: one
            // trace line per REAL GPU submit+present (emitted by
            // `flui-engine`'s `render_scene` after `present()`), counted
            // from this captured log.
            "warn,flui_widgets::scroll=trace,flui_platform=debug,flui.gpu=trace",
        )
        // BOTH streams: the app's subscriber writes to stdout — a
        // null'd stdout was why CI failures reported "stderr (last 0
        // lines)" with nothing to diagnose from.
        .stdout(log_file.try_clone().context("cloning the app log handle")?)
        .stderr(log_file)
        .spawn()
        .with_context(|| format!("spawning {app_path}"))?;

    // Ensure the child never outlives a failed run — and never fail
    // silently: the app's own stderr is the first thing a diagnosis needs
    // (a CI runner without a Vulkan adapter panics before any window
    // exists, and a nulled stderr turned that into a bare "no window").
    let result = run_checks(&mut app, &log_path);
    if result.is_err() {
        let _ = app.kill();
        let _ = app.wait();
        if let Ok(log) = std::fs::read_to_string(&log_path) {
            let tail: Vec<&str> = log.lines().rev().take(200).collect();
            eprintln!("live-smoke: app stderr (last {} lines):", tail.len());
            for line in tail.iter().rev() {
                eprintln!("  {line}");
            }
        }
    }
    let _ = std::fs::remove_file(&log_path);
    result
}

fn run_checks(app: &mut Child, app_log: &std::path::Path) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None).context("connecting to the X display")?;
    let root = conn.setup().roots[screen_num].root;

    // Check 1: the window appears.
    let window = wait_for_window(&conn, root, app)?;
    eprintln!("live-smoke: launch OK (window {window:#x})");
    // Let the first frames land before the baseline capture.
    std::thread::sleep(Duration::from_secs(2));

    // Check 2: a drag scrolls — captured pixels must change.
    let geometry = conn.get_geometry(window)?.reply()?;
    let coords = conn
        .translate_coordinates(window, root, 0, 0)?
        .reply()
        .context("window position on the root")?;
    let (win_x, win_y) = (i32::from(coords.dst_x), i32::from(coords.dst_y));
    let center_x = (win_x + i32::from(geometry.width) / 2) as i16;
    let drag_from_y = (win_y + i32::from(geometry.height) * 3 / 4) as i16;

    let before = capture(&conn, window, &geometry)?;

    fake_motion(&conn, root, center_x, drag_from_y)?;
    conn.sync()?;
    std::thread::sleep(Duration::from_millis(200));
    conn.xtest_fake_input(BUTTON_PRESS, 1, 0, root, 0, 0, 0)?;
    conn.sync()?;
    let drag_to = |from: i16, steps: i16| -> Result<i16> {
        for step in 1..=steps {
            fake_motion(&conn, root, center_x, from - step * 15)?;
            conn.sync()?;
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(from - steps * 15)
    };

    // Still held: the screen must ALREADY have moved before the release — a
    // screen that only updates on release means input stopped producing
    // frames (the parked-loop bug: 125 pointer events, 2 frames).
    // Deadline-polled, not fixed-sleep: a software raster on a 2-core CI
    // runner can take hundreds of milliseconds per frame, and a fixed gap can
    // straddle ZERO presents.
    //
    // The baseline is taken after ONE step, not after ten. Comparing the
    // second half of the drag against the first made the check depend on the
    // list still having scroll room at the END of 150 px of travel: this drag
    // starts at offset 0 and travels upward, so if the first half exhausted
    // the content the second half moved nothing and the check failed with
    // delivery perfectly healthy. How far the first half actually scrolls
    // varies with how many XTEST motions the runner coalesces under load,
    // which is why it failed intermittently (~1 in 8 on PR runs, never
    // locally, and on doc-only changes). Anchoring at the first step spans
    // nearly the whole drag and cannot start at the clamp.
    fake_motion(&conn, root, center_x, drag_from_y - 15)?;
    conn.sync()?;
    std::thread::sleep(Duration::from_millis(300));
    let mid_first = capture(&conn, window, &geometry)?;
    let _ = drag_to(drag_from_y - 15, 19)?;
    wait_for_pixel_change(&conn, window, &geometry, &mid_first).map_err(|_| {
        anyhow::anyhow!(
            "mid-drag check FAILED: pixels frozen across 285px of held drag \
             (baseline taken 15px in) — input is not producing frames"
        )
    })?;
    eprintln!("live-smoke: mid-drag tracking OK (screen follows the pointer)");

    conn.xtest_fake_input(BUTTON_RELEASE, 1, 0, root, 0, 0, 0)?;
    conn.sync()?;

    wait_for_pixel_change(&conn, window, &geometry, &before).map_err(|_| {
        anyhow::anyhow!(
            "drag check FAILED: pixels identical before and after a 300px \
             drag — pointer moves are not reaching the scrollable at all"
        )
    })?;
    eprintln!("live-smoke: drag scrolls OK (pixels changed)");

    // Wheel scrolling: three wheel-down ticks (X11 button 5) with the
    // cursor over the list must move the content further down. Let any
    // post-release fling settle first, so the poll below can only be
    // satisfied by the wheel itself — and log the settling for CI triage.
    let mut settle_probe = capture(&conn, window, &geometry)?;
    let settle_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let next = capture(&conn, window, &geometry)?;
        if next == settle_probe {
            break;
        }
        settle_probe = next;
        if Instant::now() > settle_deadline {
            eprintln!("live-smoke: note — screen still animating 15s after release");
            break;
        }
    }
    let wheel_before = capture(&conn, window, &geometry)?;
    for _ in 0..3 {
        conn.xtest_fake_input(BUTTON_PRESS, WHEEL_DOWN, 0, root, 0, 0, 0)?;
        conn.xtest_fake_input(BUTTON_RELEASE, WHEEL_DOWN, 0, root, 0, 0, 0)?;
        conn.sync()?;
        std::thread::sleep(Duration::from_millis(100));
    }
    wait_for_pixel_change(&conn, window, &geometry, &wheel_before).map_err(|_| {
        anyhow::anyhow!(
            "wheel check FAILED: pixels identical across three wheel ticks — \
             pointer-scroll dispatch is not reaching the scrollable"
        )
    })?;
    eprintln!("live-smoke: wheel scrolls OK (pixels changed)");

    // Check: hidden-surface gating under a REAL X11 occlusion signal.
    check_occlusion_gating(&conn, screen_num, root, window, app_log)?;

    // Check: a real window close exits cleanly.
    send_wm_delete(&conn, window)?;
    let status = wait_for_exit(app, EXIT_TIMEOUT)?;
    match status {
        Some(status) if status.success() => {
            eprintln!("live-smoke: clean close OK (exit 0)");
            Ok(())
        }
        Some(status) => bail!(
            "close check FAILED: teardown finished with {status} — a post-quit \
             crash (the signal a green test suite never sees)"
        ),
        None => bail!(
            "close check FAILED: still running {}s after WM_DELETE_WINDOW — \
             the close request never reached teardown, or teardown hangs",
            EXIT_TIMEOUT.as_secs()
        ),
    }
}

/// One presented frame's marker in the app log — the machine-oriented
/// `event` field of the `flui.gpu` trace `flui-engine`'s `render_scene`
/// emits immediately after the frame's encoders were submitted to the wgpu
/// queue and the swapchain texture presented (matched on the field, not the
/// human-readable message, so message rewording cannot silently break the
/// oracle). Counting occurrences counts REAL GPU submissions, which is the
/// hidden-surface criterion ("an occluded window issues zero GPU
/// submissions"), not merely "no frame produced".
const GPU_PRESENT_MARKER: &str = "event=\"present_submitted\"";
/// One pointer-scroll delivery in the app log (`flui_widgets::scroll`'s own
/// trace) — used while covered to pin the *designed* input drop: a hidden
/// presentation refuses pointer input (`Suspended` lifecycle), so wheel
/// ticks that demonstrably arrive at the translation layer must produce
/// zero of these.
const SCROLL_TICK_LINE: &str = "pointer-scroll tick";
/// One wheel delivery at the winit translation layer — proof the event loop
/// and platform translation keep running while frames are gated (the
/// "without stopping wall-clock services" half of the criterion).
const MOUSE_WHEEL_LINE: &str = "MouseWheel";

/// Hidden-surface gating, driven by a REAL platform visibility signal.
///
/// X11 delivers `VisibilityNotify(FullyObscured)` when another window fully
/// covers the app's window — winit's X11 backend translates exactly that
/// into `WindowEvent::Occluded(true)` (and any other visibility state into
/// `Occluded(false)`). Under bare Xvfb there is no compositor, so full
/// obscuration is genuinely producible — unlike a live compositing desktop,
/// where this event never fires. The cover window is override-redirect
/// (no window manager involved) with an EMPTY input region (SHAPE
/// extension): it obscures the app visually — the X server computes
/// visibility from output geometry alone — while wheel input passes
/// straight through to the app beneath, which the covered-phase probes rely
/// on.
///
/// Frame demand while hidden cannot be injected from outside: a hidden
/// presentation drops pointer input BY DESIGN (`Suspended` lifecycle —
/// asserted below as its own check), so the demand must already be in
/// flight when the surface goes hidden. A vigorous fling provides it: the
/// ballistic scroll animation is presenting frames continuously at the
/// moment the cover maps, and a broken gate would keep presenting them.
///
/// Sequence, with the wire under test spelled out:
/// `VisibilityNotify` → winit `Occluded` → flui-platform's
/// `dispatch_visibility_status_change` → `PlatformToUi::WindowVisibility` →
/// `UiRealm::set_presentation_hidden` (per-presentation `FrameClock` gate)
/// plus the `AppLifecycleState` derivation (`frames_enabled`) → the frame
/// pump stops reaching the renderer: **zero GPU submissions while covered
/// (the fling freezes), prompt resumption on uncover with no input needed**
/// (the frames-reenable redirty plus retained animation demand).
fn check_occlusion_gating(
    conn: &RustConnection,
    screen_num: usize,
    root: Window,
    window: Window,
    app_log: &std::path::Path,
) -> Result<()> {
    // X11 core button 4: wheel-up. Up, not down: every prior check scrolled
    // the list DOWN, so up-room is guaranteed while down-room may be
    // exhausted at the list's end (a clamped no-op tick creates no frame
    // demand and would starve the oracle).
    let wheel_up_tick = || -> Result<()> {
        const WHEEL_UP: u8 = 4;
        conn.xtest_fake_input(BUTTON_PRESS, WHEEL_UP, 0, root, 0, 0, 0)?;
        conn.xtest_fake_input(BUTTON_RELEASE, WHEEL_UP, 0, root, 0, 0, 0)?;
        conn.sync()?;
        Ok(())
    };
    // Stripped of ANSI escapes: the app's compact formatter colors its
    // output even into a redirected file, splitting `field=value` text
    // (`occluded` … `=` … `true` each get their own escape-wrapped span),
    // so raw substring matching silently never fires.
    let read_log = || -> Result<String> {
        Ok(strip_ansi(
            &std::fs::read_to_string(app_log).context("reading the app log")?,
        ))
    };
    let count = |hay: &str, needle: &str| hay.matches(needle).count();

    // 1. Arm the GPU oracle: the per-present trace line must demonstrably
    //    be live BEFORE anything is asserted about its absence — a filter
    //    typo or a renamed message must fail here, loudly, not let the
    //    covered-window assertion pass vacuously.
    let gpu_visible_base = count(&read_log()?, GPU_PRESENT_MARKER);
    wheel_up_tick()?;
    wheel_up_tick()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if count(&read_log()?, GPU_PRESENT_MARKER) > gpu_visible_base {
            break;
        }
        if Instant::now() > deadline {
            bail!(
                "occlusion check FAILED (oracle arming): no '{GPU_PRESENT_MARKER}' log line \
                 after wheel input on a visible window — the flui.gpu trace target is \
                 not reaching the log, so nothing below could have been asserted"
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // Post-arming baseline, taken only once presents have QUIESCED: the
    // arming ticks above produce submissions of their own, so a baseline
    // captured before them would let those ticks satisfy the fling premise
    // below even if the drag never started a fling — a vacuous pass.
    // Growth beyond THIS baseline is attributable to the drag/fling alone.
    // Quiescence is best-effort (bounded): a stuck-animating app weakens
    // attribution but cannot weaken the gate assertion itself.
    //
    // Re-established before EVERY drag attempt, not once: it is what makes
    // the reported frame count attributable to the attempt that actually
    // ran. (The premise itself no longer rests on this baseline — liveness
    // is measured against the pre-release sample of the same attempt — so a
    // stale baseline can no longer manufacture a vacuous pass either way.)
    let quiesced_present_baseline = || -> Result<usize> {
        let mut base = count(&read_log()?, GPU_PRESENT_MARKER);
        let quiesce_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            std::thread::sleep(Duration::from_millis(300));
            let sample = count(&read_log()?, GPU_PRESENT_MARKER);
            let stable = sample == base;
            base = sample;
            if stable || Instant::now() > quiesce_deadline {
                break;
            }
        }
        Ok(base)
    };

    // 2. Prepare (but do not yet map) the cover: an input-transparent
    //    override-redirect window spanning the whole screen. Created ahead
    //    of the fling so mapping it later is a single round-trip.
    let screen = &conn.setup().roots[screen_num];
    let cover = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        cover,
        root,
        0,
        0,
        screen.width_in_pixels,
        screen.height_in_pixels,
        0,
        WindowClass::INPUT_OUTPUT,
        0, // CopyFromParent visual
        &CreateWindowAux::new()
            .override_redirect(1)
            .background_pixel(screen.black_pixel),
    )?;
    // EMPTY input region: zero rectangles. Visual obscuration (and thus
    // VisibilityNotify) is unaffected — the server derives it from output
    // geometry — but pointer events fall through to the app beneath.
    conn.shape_rectangles(
        shape::SO::SET,
        shape::SK::INPUT,
        ClipOrdering::UNSORTED,
        cover,
        0,
        0,
        &[],
    )?;

    // 3. Launch a vigorous UPWARD drag-fling — finger up, so the content
    //    scrolls DOWN and the offset grows.
    //
    //    The direction is load-bearing, and the opposite one is a real trap
    //    this check fell into: the arming ticks above are wheel-UP, which
    //    walks the offset back toward the top (observed: 318 -> 106 px, two
    //    requested ticks arriving as four scroll events). A downward
    //    drag-fling needs room in that SAME direction, so it would be
    //    dragging into the ~106 px the arming just left — 240 px of motion
    //    against a 106 px wall clamps almost immediately, the release
    //    carries no ballistic velocity, and the premise below fails with a
    //    dead fling while pointer delivery is perfectly healthy. Dragging
    //    the other way instead consumes the room the arming CREATED: the
    //    demo is 30 rows (~1.9 k px of content in an 800 px viewport), so
    //    near the top there is ~1 k px of down-room — an order of magnitude
    //    more than this drag needs, independent of where earlier checks
    //    happened to leave the offset.
    //
    //    Starting low (3/4 of the way down) keeps all eight upward steps
    //    inside the window; starting at 1/4 would walk the pointer off the
    //    top edge and silently truncate the gesture.
    //
    //    The drag is RETRIED, because its premise depends on something the
    //    harness cannot control: whether the app's event loop drained each
    //    motion promptly. Timestamps on this backend are RECEIPT times, not
    //    input times — winit's `CursorMoved` carries no timestamp, so
    //    `flui_platform`'s winit `pointer_state` stamps
    //    `PROCESS_START.elapsed()` when the event reaches FLUI. A stalled
    //    loop therefore corrupts the velocity samples even though every
    //    motion was delivered and applied, and `VelocityTracker` answers
    //    with `Offset::ZERO`: either a delivery gap crosses its 40 ms
    //    `ASSUME_POINTER_STOPPED` contiguity cliff, or a batched drain
    //    compresses the whole drag into one timestamp. Both leave a moved
    //    viewport with no ballistic release, which is precisely the failure
    //    this premise reports.
    //
    //    Retrying the PREMISE is not retrying the assertion. The occlusion
    //    gate below still has to hold on its own; all these attempts buy is
    //    a live animation for it to be asserted against.
    let geometry = conn.get_geometry(window)?.reply()?;
    let coords = conn
        .translate_coordinates(window, root, 0, 0)?
        .reply()
        .context("window position on the root")?;
    let center_x = (i32::from(coords.dst_x) + i32::from(geometry.width) / 2) as i16;
    let fling_start_y = (i32::from(coords.dst_y) + i32::from(geometry.height) * 3 / 4) as i16;

    // Eight steps of 30 px at 12 ms rather than six of 40 px at 8 ms. Same
    // 240 px of travel and the same direction; the point is the SPACING.
    // Denser sampling gives the tracker more than its three-sample minimum
    // to work with, and a 12 ms cadence leaves the loop a frame's worth of
    // time to drain each motion separately instead of batching them —
    // while staying well clear of the 40 ms contiguity cliff that a longer
    // pause would walk into.
    let drag_once = || -> Result<(bool, usize, usize)> {
        fake_motion(conn, root, center_x, fling_start_y)?;
        conn.sync()?;
        std::thread::sleep(Duration::from_millis(100));
        // Witnesses for the premise below. The drag itself writes nothing to
        // the log — only the wheel path logs `pointer-scroll tick` — so when
        // the premise fails with a dead fling, the log alone cannot say
        // whether the gesture engaged at all. These two make that distinction
        // reportable instead of leaving the next reader to guess.
        let before_drag_pixels = capture(conn, window, &geometry)?;
        let gpu_before_drag = count(&read_log()?, GPU_PRESENT_MARKER);
        conn.xtest_fake_input(BUTTON_PRESS, 1, 0, root, 0, 0, 0)?;
        conn.sync()?;
        for step in 1..=8i16 {
            fake_motion(conn, root, center_x, fling_start_y - step * 30)?;
            conn.sync()?;
            std::thread::sleep(Duration::from_millis(12));
        }
        // Sampled BEFORE the release: every marker counted here presented
        // during (or before) the drag, so any count above it afterwards is a
        // frame the still-running ballistic animation presented. That is the
        // liveness evidence the poll below waits for.
        let pre_release = count(&read_log()?, GPU_PRESENT_MARKER);
        conn.xtest_fake_input(BUTTON_RELEASE, 1, 0, root, 0, 0, 0)?;
        conn.sync()?;
        let after_drag_pixels = capture(conn, window, &geometry)?;
        let drag_moved_content = after_drag_pixels != before_drag_pixels;
        let drag_frames = count(&read_log()?, GPU_PRESENT_MARKER).saturating_sub(gpu_before_drag);
        Ok((drag_moved_content, drag_frames, pre_release))
    };

    // 4. Cover the app mid-fling — LIVENESS-gated, never time-gated and
    //    never count-gated. A fixed drag->cover gap is runner-speed
    //    dependent, and so is any ABSOLUTE frame threshold: on a slow CI
    //    software raster the whole ballistic decay spans only a couple of
    //    frame intervals, so "N frames beyond the baseline" is unreachable
    //    there however long the poll waits, and retrying the fling cannot
    //    help — the ubuntu runner was observed presenting 2 frames in
    //    total. What the vacuity property actually needs is that demand
    //    exists AT cover time, and liveness proves that on any frame
    //    budget: poll until at least one frame lands AFTER the sample taken
    //    just before the button release, i.e. the animation presented
    //    within the last poll interval, then map the cover immediately.
    //    Only the bound expiring with no post-release frame at all is a
    //    failure: a genuinely dead fling.
    //
    //    Each attempt gets its own freshly quiesced baseline, so a later
    //    attempt can never inherit a dead earlier one's frames.
    const FLING_ATTEMPTS: u32 = 3;
    let mut fling_frames = 0usize;
    let mut established = false;
    // Carried out of the loop so the failure below reports the LAST
    // attempt's evidence rather than a placeholder.
    let mut drag_moved_content = false;
    let mut drag_frames = 0usize;
    let mut frames = 0usize;

    for attempt in 1..=FLING_ATTEMPTS {
        let gpu_armed_base = quiesced_present_baseline()?;
        let outcome = drag_once()?;
        drag_moved_content = outcome.0;
        drag_frames = outcome.1;
        let pre_release = outcome.2;

        // The first attempt keeps the original 10s bound; retries are
        // shorter, since a fling that has not appeared in 6s on an already
        // warm app is not going to.
        let bound = if attempt == 1 {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(6)
        };
        let fling_deadline = Instant::now() + bound;
        loop {
            std::thread::sleep(Duration::from_millis(150));
            let sample = count(&read_log()?, GPU_PRESENT_MARKER);
            frames = sample.saturating_sub(gpu_armed_base);
            if sample > pre_release {
                fling_frames = frames;
                established = true;
                break;
            }
            if Instant::now() > fling_deadline {
                break;
            }
        }
        if established {
            break;
        }
        eprintln!(
            "live-smoke: drag-fling attempt {attempt}/{FLING_ATTEMPTS} produced no live \
             animation ({frames} frame(s) past baseline, drag itself {drag_frames}); retrying"
        );
    }

    if !established {
        // Two different defects reach this line, and the frame count alone
        // does not separate them, so say which one this was. The drag
        // either moved the content (the gesture engaged and the release
        // simply carried no ballistic velocity) or it did not (the press
        // never became a scroll gesture — hit-testing, arena, or event
        // delivery), and a reader who cannot tell them apart is left
        // guessing at the whole subsystem.
        let verdict = if drag_moved_content {
            "the drag DID move the content, so the gesture engaged and the \
             release carried no ballistic velocity — look at the drag's \
             velocity samples, not at delivery. The app logs those samples: \
             grep the captured log for `velocity estimate` and read \
             `contiguous_samples` and `span_ms` — fewer than 3 samples means a \
             delivery gap crossed the 40 ms contiguity cliff, while 3 or more \
             over a near-zero span means a batched drain collapsed the drag \
             into one receipt timestamp"
        } else {
            "the drag did NOT move the content at all, so the press never \
             became a scroll gesture — look at hit-testing, the gesture \
             arena, or event delivery, not at the fling"
        };
        bail!(
            "occlusion check FAILED (premise): {FLING_ATTEMPTS} drag-fling attempts each \
             presented only a few frames beyond their own quiesced post-arming baseline \
             (last attempt: {frames}) — no live animation exists for the gate to stop, so \
             the zero-submissions assertion would be vacuous.\n  Last drag itself presented \
             {drag_frames} frame(s). {verdict}."
        );
    }
    eprintln!("live-smoke: drag-fling live with {fling_frames} presented frames — covering now");
    conn.map_window(cover)?;
    conn.sync()?;

    // The app must OBSERVE the occlusion: winit's translation logs
    // "Window occlusion changed … occluded=true" at debug level. A timeout
    // here means the platform wire is dead (or this X server did not
    // deliver VisibilityNotify — bare Xvfb always does).
    let occluded_line =
        |log: &str| log.contains("Window occlusion changed") && log.contains("occluded=true");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if occluded_line(&read_log()?) {
            break;
        }
        if Instant::now() > deadline {
            bail!(
                "occlusion check FAILED: the app never observed Occluded(true) within 10s \
                 of being fully covered — the platform visibility wire is dead (or \
                 VisibilityNotify was not delivered, which bare Xvfb always does)"
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("live-smoke: occlusion signal observed by the app (Occluded=true)");

    // 5. Let the in-flight frame (if any) drain, then snapshot. Everything
    //    in the log AFTER this point happened while the app knew it was
    //    occluded.
    std::thread::sleep(Duration::from_millis(500));
    let hidden_snapshot = read_log()?;
    let gpu_hidden_base = count(&hidden_snapshot, GPU_PRESENT_MARKER);
    let scroll_hidden_base = count(&hidden_snapshot, SCROLL_TICK_LINE);
    let wheel_hidden_base = count(&hidden_snapshot, MOUSE_WHEEL_LINE);

    // The fling premise was already established BEFORE the cover mapped
    // (the liveness-gated poll above): the animation presented a frame
    // within the poll interval preceding the cover, so the
    // zero-submissions assertion below is non-vacuous by construction. (`gpu_visible_base`, before
    // arming, only feeds the arming loop above.)

    // 6. Covered-phase probes: two wheel ticks pass through the cover's
    //    empty input region.
    for _ in 0..2 {
        wheel_up_tick()?;
        std::thread::sleep(Duration::from_millis(300));
    }
    std::thread::sleep(Duration::from_millis(600));
    let covered_log = read_log()?;

    // Wall-clock half of the criterion: the event loop and platform
    // translation must still be running while frames are gated — the ticks
    // must show up at the translation layer.
    if count(&covered_log, MOUSE_WHEEL_LINE) <= wheel_hidden_base {
        bail!(
            "occlusion check FAILED: no '{MOUSE_WHEEL_LINE}' translation lines while \
             covered — the event loop stopped servicing platform events entirely \
             while hidden (gating must stop FRAMES, not wall-clock services)"
        );
    }
    // Designed input drop: a hidden presentation refuses pointer input
    // (`Suspended` lifecycle), so those delivered ticks must produce ZERO
    // scrollable dispatches.
    if count(&covered_log, SCROLL_TICK_LINE) > scroll_hidden_base {
        bail!(
            "occlusion check FAILED: pointer-scroll dispatches reached the scrollable \
             while the presentation was hidden — the Suspended-lifecycle input drop \
             is not being applied"
        );
    }

    // THE assertion: zero GPU submissions while the surface was hidden,
    // with a live fling as demand.
    let gpu_covered = count(&covered_log, GPU_PRESENT_MARKER);
    if gpu_covered != gpu_hidden_base {
        bail!(
            "occlusion check FAILED: {} GPU submission(s) while the window was fully \
             occluded — the hidden-surface gate is not stopping the frame pump \
             (Occluded(true) was observed, so the signal arrived; the gate did not act)",
            gpu_covered - gpu_hidden_base
        );
    }
    eprintln!("live-smoke: hidden-surface gating OK (zero GPU submissions while covered)");

    // 7. Uncover. Visibility must be re-observed AND frames must resume
    //    WITHOUT any further input: the frames-reenable redirty repaints
    //    unconditionally on the disabled→enabled edge, and the frozen
    //    fling's retained demand rides the same wake. (Nudging the app
    //    here would mask a broken resume wake.)
    conn.destroy_window(cover)?;
    conn.sync()?;
    let unoccluded_line =
        |log: &str| log.contains("Window occlusion changed") && log.contains("occluded=false");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = read_log()?;
        if unoccluded_line(&log) && count(&log, GPU_PRESENT_MARKER) > gpu_hidden_base {
            break;
        }
        if Instant::now() > deadline {
            let log = read_log()?;
            if !unoccluded_line(&log) {
                bail!(
                    "occlusion check FAILED: the app never observed Occluded(false) \
                     within 10s of the cover being destroyed"
                );
            }
            bail!(
                "occlusion check FAILED: Occluded(false) was observed but no GPU \
                 submission followed within 10s — the unhide wake / frames-reenable \
                 redirty is not resuming the frame pump"
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("live-smoke: unhide resume OK (frames resumed with no further input)");
    Ok(())
}

/// Remove ANSI CSI escape sequences (`ESC [ … <final byte in @..=~>`) —
/// everything the tracing compact formatter emits for color.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if ('@'..='~').contains(&c2) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Poll the window tree for a window whose `WM_NAME` contains "FLUI",
/// failing early if the app dies first. Breadth-first to bounded depth: a
/// window manager (any environment other than bare Xvfb) reparents client
/// windows under decoration frames, so root's DIRECT children are not
/// enough.
fn wait_for_window(conn: &RustConnection, root: Window, app: &mut Child) -> Result<Window> {
    const MAX_DEPTH: usize = 4;
    let deadline = Instant::now() + WINDOW_TIMEOUT;
    loop {
        if let Some(status) = app.try_wait()? {
            bail!("launch check FAILED: the app exited during startup with {status}");
        }
        let mut frontier = vec![(root, 0usize)];
        while let Some((candidate, depth)) = frontier.pop() {
            // `AtomEnum::ANY` (type 0): accept STRING and UTF8_STRING alike.
            let name = conn
                .get_property(false, candidate, AtomEnum::WM_NAME, AtomEnum::ANY, 0, 256)?
                .reply()?;
            if String::from_utf8_lossy(&name.value).contains("FLUI") {
                return Ok(candidate);
            }
            if depth < MAX_DEPTH {
                for &child in &conn.query_tree(candidate)?.reply()?.children {
                    frontier.push((child, depth + 1));
                }
            }
        }
        if Instant::now() > deadline {
            bail!(
                "launch check FAILED: no FLUI window within {}s",
                WINDOW_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn fake_motion(conn: &RustConnection, root: Window, x: i16, y: i16) -> Result<()> {
    conn.xtest_fake_input(MOTION_NOTIFY, 0, 0, root, x, y, 0)?;
    Ok(())
}

/// Poll until the window's pixels differ from `baseline` — the deadline
/// absorbs arbitrarily slow software rasters without weakening the oracle
/// (an unchanged screen still fails, just after the full deadline).
fn wait_for_pixel_change(
    conn: &RustConnection,
    window: Window,
    geometry: &x11rb::protocol::xproto::GetGeometryReply,
    baseline: &[u8],
) -> Result<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = capture(conn, window, geometry)?;
        if current != baseline {
            return Ok(current);
        }
        if Instant::now() > deadline {
            bail!("no pixel change within the deadline");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The window's current pixels, straight from the server.
fn capture(
    conn: &RustConnection,
    window: Window,
    geometry: &x11rb::protocol::xproto::GetGeometryReply,
) -> Result<Vec<u8>> {
    let image = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            window,
            0,
            0,
            geometry.width,
            geometry.height,
            !0,
        )?
        .reply()
        .context("capturing window pixels")?;
    Ok(image.data)
}

/// A real window close: the ICCCM `WM_DELETE_WINDOW` client message —
/// exactly what a window manager sends for the titlebar button, and the
/// path that reaches winit's `CloseRequested`. (Destroying the window
/// outright would bypass the app's teardown entirely.)
fn send_wm_delete(conn: &RustConnection, window: Window) -> Result<()> {
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    let delete = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    let event = ClientMessageEvent::new(32, window, protocols, [delete, 0, 0, 0, 0]);
    conn.send_event(false, window, EventMask::NO_EVENT, event)?;
    conn.sync()?;
    Ok(())
}

fn wait_for_exit(app: &mut Child, timeout: Duration) -> Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = app.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() > deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
