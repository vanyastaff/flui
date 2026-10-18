//! End-to-end checks on a real OS: build a probe, stage it the way the
//! platform needs it, and run it or its driver.
//!
//! The Apple drivers stay Python and Swift under `tools/device-checks/`: they
//! only run on a Mac, so only a Mac can port them with evidence. The Windows
//! checks are clients of Win32 APIs xtask can call itself, so they run
//! in-process (`device/windows_a11y.rs`, `device/windows_input.rs`, Windows
//! only). This module owns the build, the staging and the verdict on a probe's
//! output as a plan computed first and executed second, so a test on any host
//! can hold each check against its steps.
//!
//! On another host a check says why it is skipped and exits 0.
//! `macos-workload` and `macos-hot-reload-loop` leave that decision to their
//! drivers (the first skips with 0, the second refuses with 1). Paths are
//! relative to the repository root, where every step runs.

mod android;
mod android_gallery;
mod android_input;
mod bidi;
mod chrome;
mod gallery;
mod plan;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "a UI Automation client is COM calls through the `windows` bindings"
)]
mod uia;
#[cfg_attr(
    not(windows),
    expect(
        dead_code,
        reason = "read by the Windows UI Automation check; its guards run on every host"
    )
)]
mod uia_role_table;
mod web_gallery;
mod web_input;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "a UI Automation client is COM calls through the `windows` bindings"
)]
mod windows_a11y;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "the gallery driver is the same Windows COM and input boundary"
)]
mod windows_gallery;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "OS input is `SendInput` through the `windows` bindings"
)]
mod windows_input;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "native Notes uses the same Windows COM and input boundary"
)]
mod windows_notes;
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "screen captures and synthetic touch are GDI and pointer-injection calls"
)]
mod windows_screen;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use plan::{Announce, Native, Program, Run, Sink, Step};

use crate::util::{metadata, repo_root};

/// The Rust target the simulator builds use.
const IOS_SIM_TARGET: &str = "aarch64-apple-ios-sim";
/// The simulator `ios-sim` boots when `FLUI_IOS_SIM_DEVICE` names none.
const DEFAULT_SIM_DEVICE: &str = "iPhone 17 Pro";
/// The bundle identifier `examples/Info.plist.ios_demo` declares.
const IOS_DEMO_BUNDLE: &str = "dev.flui.ios-demo";
const INPUT_DRIVER: &str = "tools/device-checks/check-ios-input.py";

/// Arguments for `cargo xtask device`.
#[derive(Debug, clap::Args)]
pub(crate) struct DeviceArgs {
    #[command(subcommand)]
    check: DeviceCheck,
}

/// `cargo xtask device <check>`: run a macOS, iOS or Windows device check.
pub(crate) fn device(args: &DeviceArgs) -> anyhow::Result<ExitCode> {
    if !args.check.runs_on_this_host()
        && let Some(reason) = args.check.skip_reason()
    {
        println!("{reason}");
        return Ok(ExitCode::SUCCESS);
    }
    let root = repo_root();
    let context = Context::from_host(&root)?;
    plan::execute(&args.check.plan(&context), &root)
}

/// A device check.
#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand)]
enum DeviceCheck {
    /// AppKit close and teardown on a real Mac (issue #1148, AppKit half).
    ///
    /// Builds the `close_path_probe` example and stages it into a minimal
    /// `.app` bundle: the committed Info.plist clears
    /// `_CFBundleGetValueForInfoKey`, and the binary's `fn main` is the AppKit
    /// main thread — the two floors that make unbundled libtest unable to host
    /// real AppKit windows. Runs it with `RUST_LOG=info` and requires exit 0
    /// plus the `CLOSE_PATH_PROBE_RESULT=PASS` marker.
    MacosClosePath,
    /// AppKit frame pump on a real Mac.
    ///
    /// Builds the `frame_pump_probe` example, stages it into a minimal `.app`
    /// the same way `macos-close-path` does, runs it with `RUST_LOG=info`, and
    /// requires exit 0 plus the `FRAME_PUMP_PROBE_RESULT=PASS` marker. The
    /// probe counts frames from a real visible window whose frame callback
    /// re-arms itself the way the engine's frame does, and requires frames to
    /// keep arriving after the primer that started them stopped: the AppKit
    /// display pass discards an in-pass `setNeedsDisplay:`, so a backend
    /// without the deferral runs exactly one frame and reports FAIL.
    MacosFramePump,
    /// Resize transients on a real Mac.
    ///
    /// Builds the `resize_jitter_probe` example into a staged `.app` the same
    /// way `macos-frame-pump` does, runs it with `RUST_LOG=info`, and requires
    /// exit 0 plus the `RESIZE_JITTER_PROBE_RESULT=PASS` and
    /// `RESIZE_JITTER_PROBE_STALE=0` markers. The probe drives a scripted burst
    /// of real window resizes while rendering continuously into the Metal
    /// swapchain, with the surface deliberately held frames behind the window,
    /// and counts `Renderer::warn_on_size_mismatch` — the acquired swapchain
    /// texture differing from the configured surface size, i.e. the frame a
    /// compositor would stretch. It pins that invariant; it does not
    /// discriminate `desired_maximum_frame_latency`, which it was built to do
    /// and measurably cannot (zero at 1 and at 2, four runs) — see the probe's
    /// own module doc and the literal's comment in renderer.rs.
    MacosResizeJitter,
    /// Text input on a real Mac.
    ///
    /// Builds the `ime_probe` example, stages it into a `.app` the same way
    /// `macos-frame-pump` does, runs it with `RUST_LOG=info`, and requires
    /// exit 0 plus the `IME_PROBE_RESULT=PASS` marker. The probe runs the real
    /// backend through the production launch path, reaches the window's
    /// content view through AppKit, and drives four assertions: (A, ADR-0069)
    /// one synthesized keyDown for one letter reaches the application as
    /// exactly one `ImeEvent::Commit` with zero `Key::Character` while a text
    /// input is attached, and the exact inverse with it detached; (B) the
    /// `NSTextInputClient` queries AppKit makes answer correctly, including the
    /// UTF-16 to byte cursor conversion; (C) a cursor area set through the
    /// trait comes back as a non-zero rect; (D) `unmarkText` announces the end
    /// of composition. The key events are synthesized, not human keystrokes,
    /// and no genuine input method runs, so a real composition stays undriven:
    /// the probe covers the routing and the protocol, not the input method.
    MacosIme,
    /// Launch-route rendering on a real Mac, judged by the window's pixels.
    ///
    /// Builds the `colored_box_app` example and runs it through
    /// `tools/device-checks/check-macos-launch-render.py`, which launches the
    /// same bundled artifact three ways (direct exec, `open` / LaunchServices,
    /// `open -g` / LaunchServices without activation), 5 launches each, finds
    /// each launch's window by owning PID in the CoreGraphics window list, and
    /// photographs it by window number. The oracle is the pixels of that
    /// window, not frames or survival: the fixture paints pure red, and every
    /// launch must show it, because a window can exist, hold a live frame
    /// pump, and still be blank. A genuinely blank window is the control and
    /// fails (see the checker's own validation), so the gate discriminates
    /// instead of merely passing.
    ///
    /// Each launch is given a bounded settle, because macOS orders a window
    /// front before its first frame is presented: the oracle is retried until
    /// it holds, so a window that is merely early is not mistaken for a blank
    /// one, and the time to the capture that passed is reported as first frame
    /// after. A window-scoped capture of an unpainted window is a flat dark
    /// image whatever the display shows, so on a failure the checker also
    /// takes one screen capture of the window rectangle, only when that window
    /// is frontmost, and reports what a viewer had on screen, as a diagnostic
    /// that decides nothing.
    ///
    /// Needs Screen Recording, which is preflighted: without it the checker
    /// exits 2 (cannot verify) rather than reporting a blank window it never
    /// saw. This closes the LaunchServices half of the blank-window
    /// observation in docs/BETA.md.
    MacosLaunchRender,
    /// Window-lifecycle budget on a real Mac (docs/BETA.md's lifecycle rows).
    ///
    /// Builds `examples/lifecycle_probe` in release and runs it. The probe runs
    /// a Material tree through the ordinary Application path, then drives its
    /// own window through AppKit from a driver thread —
    /// miniaturize/deminiaturize, hide/unhide the application,
    /// `setFrame:display:` — with no operator input, counting the frames the
    /// runner produces through each transition (a minimized or hidden window
    /// must cost at most 5 frames in 3 s; a restored one must run at least 30
    /// in 2 s) and checking that the resize reaches layout as the window's new
    /// content size. Prints one JSON line per phase and
    /// `LIFECYCLE_PROBE_RESULT=PASS` or `FAIL`.
    MacosLifecycle,
    /// Assistive technology on a real Mac (docs/BETA.md's accessibility gap).
    ///
    /// Runs `tools/device-checks/check-macos-a11y.py`, which builds
    /// `examples/a11y_probe` — the generated counter with the facade's `a11y`
    /// feature, so the AccessKit adapter is installed — runs it on a real
    /// window, and runs `tools/device-checks/macos-ax-client.swift` against the
    /// process: an `AXUIElement` client (what every macOS screen reader uses)
    /// that reads the window's accessibility tree, finds the Increment button
    /// by label, performs `AXPress`, and reads the count back as static text.
    /// No pointer or keyboard event is synthesised; if the count advances, a
    /// VoiceOver user could press the button. Exits 2 (cannot verify) when
    /// this process is not trusted for accessibility, since a denied query
    /// decides nothing.
    MacosA11y,
    /// Representative-workload budget on a real Mac (docs/BETA.md's
    /// "Performance and resilience" row).
    ///
    /// Runs `tools/device-checks/check-macos-workload.py`, which builds
    /// `examples/workload_probe` in release and samples RSS while the probe
    /// drives itself — 20 s of scrolling a 2,000-row `ListView::builder`, 500
    /// characters typed into a Material `TextField`, 5 s of enforced idleness
    /// — with no operator input and no synthetic OS events, then applies the
    /// budgets the script declares in its header (scroll/type p99 within 2
    /// display periods, under 1 % of scroll frames over 2 periods, RSS growth
    /// under 10 %, at most 5 idle frames). Every run writes
    /// `target/workload/<timestamp>.json` with the raw phase lines, the RSS
    /// series and the verdict per budget; a failed budget is a finding to
    /// record in BETA.md, not a number to tune. On another host the driver
    /// skips with exit 0.
    MacosWorkload,
    /// `flui run`'s worker hot-reload loop on a real Mac.
    ///
    /// Builds `flui-cli` and drives the loop on a freshly generated
    /// `--hot-reload` project through the CLI's `--json` event stream
    /// (`tools/device-checks/check-hot-reload-loop.py`): a label edit reloads
    /// in place (a witness the edit adds bumps and prints the host-owned
    /// counter), a syntax error is refused with the host alive, the fix
    /// reloads with the counter preserved, idle stays idle, Ctrl-C exits.
    /// Needs a macOS GUI session; the host window is brought to front before
    /// each edit because a hidden window defers the rebuild. On another host
    /// the driver refuses with exit 1.
    MacosHotReloadLoop {
        /// Scratch directory the driver recreates.
        #[arg(default_value = "target/hot-reload-loop/work")]
        work: PathBuf,
    },
    /// The iOS demo on an iOS Simulator: the only executing coverage of the
    /// native UIKit backend.
    ///
    /// Builds `examples/ios_demo` for aarch64-apple-ios-sim, stages it into a
    /// minimal `.app`, boots a simulator (`FLUI_IOS_SIM_DEVICE`, default
    /// "iPhone 17 Pro"), installs and launches it, captures a screenshot, and
    /// requires that the app got as far as a created Metal device and a
    /// rendered frame — read out of the simulator's unified log, since
    /// `UIApplicationMain` owns the process and no test harness can. A second
    /// arm does the same for an animated app and requires its pixels to move.
    /// Needs Xcode and `xcrun simctl`.
    IosSim,
    /// iOS touch and resume on the already booted simulator `udid`.
    ///
    /// Builds the Material demo for aarch64-apple-ios-sim, stages it into a
    /// minimal `.app`, and drives it through
    /// `tools/device-checks/check-ios-input.py`. The instrument is XCUITest,
    /// because nothing else can put a UITouch into the application: simctl
    /// has no touch subcommand, and host UI automation needs the Accessibility
    /// grant (and photographs the host's desktop) — XCUITest synthesizes the
    /// touch inside the simulator through the platform's own automation
    /// channel. The oracle is pixels, and has to be: the iOS backend publishes
    /// no accessibility tree, so a widget cannot be read by identifier.
    ///
    /// A real tap on a list row must change the displayed selection,
    /// Home-then-return must still display it, and two controls must behave: a
    /// fresh launch resets it (so the return comparison could have failed) and
    /// a tap on no target changes nothing (so the tap comparison distinguishes
    /// a hit from any touch). The demo's list rows are idempotent — tapping an
    /// already-selected row changes nothing — so retention here rests on
    /// display equality alone; a subject whose display keeps advancing can be
    /// held to the stronger oracle with `--post-return-tap` (see
    /// `ios-input-check-app`), which proves the resumed screen is live rather
    /// than the system's snapshot of the pre-Home frame. Exits 2 (cannot
    /// verify) when the host cannot take the measurement. This closes the
    /// "simulator UI automation timed out" gap in docs/BETA.md.
    IosInputCheck {
        /// An already booted simulator.
        udid: String,
    },
    /// The `ios-input-check` gate against an arbitrary staged `.app`.
    ///
    /// For candidates the CLI built rather than the in-repo demo: `app` is an
    /// already-staged `.app` directory (xcodebuild installs it), `udid` an
    /// already booted simulator, and the remaining arguments go to
    /// `tools/device-checks/check-ios-input.py`. Tap geometry is a property of
    /// the application, so a candidate whose widgets are not the demo's needs
    /// it. For the generated sole-flui counter, whose Increment button is at
    /// normalized y 0.097 and whose only changing text sits directly under the
    /// status-bar clock (so the region must be a centre band, or it crops out
    /// exactly what the tap changes):
    ///
    /// `cargo xtask device ios-input-check-app <app> <udid> --target-tap 0.5,0.097 --empty-tap 0.5,0.5 --region 0.25,0.0,0.75,0.96 --post-return-tap`
    ///
    /// The last of those is the stronger resume oracle and the counter is the
    /// subject that can carry it: its button accumulates (0 -> 1 -> 2), so a
    /// tap after Home/return must advance the display, which a system snapshot
    /// of the pre-Home frame never does. It cannot be used on an idempotent
    /// subject — the demo's rows — where a second tap on the same row changes
    /// nothing and the requirement would fail a correct application; there the
    /// run's report names display-equality as the oracle that carried the
    /// claim. The driver documents why each of those arguments exists.
    IosInputCheckApp {
        /// The staged `.app` directory.
        app: PathBuf,
        /// An already booted simulator.
        udid: String,
        /// Arguments for the driver.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// iOS safe-area layout on the already booted simulator `udid`.
    ///
    /// Builds the sole-facade fixture for aarch64-apple-ios-sim through
    /// `tools/device-checks/check-ios-safe-area.py` and runs it, asserting the
    /// marker the application writes after comparing both laid-out geometries
    /// against the view's own `safeAreaInsets`. Evidence belongs in
    /// docs/BETA.md § "iOS safe-area layout". Needs Xcode, the
    /// aarch64-apple-ios-sim target and a booted arm64 simulator.
    IosSafeAreaCheck {
        /// An already booted simulator.
        udid: String,
    },
    /// Assistive technology on a real Windows desktop.
    ///
    /// Builds `examples/a11y_probe` (the generated counter with the facade's
    /// `a11y` feature, so the AccessKit UIA adapter is installed), runs it on
    /// a real window, and acts as the screen reader itself through UI
    /// Automation, the API Narrator, NVDA and JAWS use: finds the window by
    /// the probe's process id, requires the prompt, the count and the
    /// Increment button in the raw tree under the names a screen reader
    /// speaks, invokes the button through `IUIAutomationInvokePattern`, and
    /// requires the count's name to become "1". No pointer or keyboard event
    /// is synthesised. The oracle is names rather than presence: its first run
    /// found both texts in the tree with empty names. Exits 2 (cannot verify)
    /// when UI Automation cannot be instantiated. Needs an interactive
    /// desktop session.
    WindowsA11y,
    /// Real pointer and keyboard input on a real Windows window, and the
    /// golden native recordings of what the backend made of it.
    ///
    /// First records two injected sessions, each in its own process through
    /// `flui-platform`'s native window contract built with `native-capture`
    /// (a debug build): a touch and a pen from `InjectSyntheticPointerInput`
    /// (`a_captured_native_pointer_session_replays_to_what_it_delivered`),
    /// and mouse clicks, a double click, a chord, a Control-held click and
    /// both wheels from `SendInput`
    /// (`a_captured_native_mouse_session_replays_to_what_it_delivered`). Each
    /// session's journal must replay through the shared decoder to exactly
    /// what the window delivered, and to the shape of its golden in
    /// `crates/flui-platform/tests/native_recording/` (the replayed events
    /// with times left out and positions in device pixels); the fresh
    /// recordings land in `target/native-recordings/` for a diff of the rows.
    /// With `--bless` the fresh recordings replace the goldens instead. A
    /// precision touchpad has no injector, so its DirectManipulation golden
    /// comes from hardware only.
    ///
    /// Then builds the same `a11y_probe` and drives it with `SendInput` — events
    /// entering the system input queue as a device's would, hit-tested and
    /// routed by the OS — while UI Automation only locates the button and
    /// reads the count: a click on the window outside the button must leave
    /// the count at "0" (the control), a click on the button must make it
    /// "1", and Tab then Enter must make it "2". Before every event the
    /// probe's window must be the foreground window, so no keystroke can
    /// reach another application; when Windows will not bring it forward the
    /// check exits 2 (cannot verify). Moves the cursor and restores it. Needs
    /// an interactive desktop session; do not touch the mouse or keyboard
    /// while it runs.
    WindowsInput {
        /// Replace the golden recordings with this run's instead of holding
        /// the run against them.
        #[arg(long)]
        bless: bool,
    },
    /// The existing Notes tree driven by real input and current UIA observations.
    WindowsNotes,
    /// The interaction gallery on a real Windows window, every screen
    /// driven by real input and each outcome read through UI Automation and
    /// the window's pixels.
    ///
    /// Builds `examples/interaction_gallery` (release, with `a11y`), opens
    /// each screen from its sidebar and drives it with `SendInput` mouse
    /// clicks, drags, wheel notches and keys and with touch contacts from a
    /// synthetic touchscreen (`InjectSyntheticPointerInput`): swipe actions
    /// and dismissal, pull to refresh, the collapsing header, the pager, the
    /// bottom sheet's anchors, zoom and pan, drag and drop, the context
    /// menu, the gesture pad and keyboard focus. After each action it reads
    /// the outcome from the UIA tree (the screen's readout, a row gone, a
    /// menu open, the focused element) and from pixels sampled where the
    /// right and the wrong outcome differ. Every check is reported; the run
    /// fails if any did. Captures of each screen land in
    /// `target/interaction-gallery/`. Exits 2 when the host refuses the
    /// foreground or the input. Needs an interactive desktop session; do not
    /// touch the mouse or keyboard while it runs. `FLUI_GALLERY_CHECKS=<text>`
    /// runs only the checks whose name contains `<text>` (`focus:`).
    WindowsGallery,
    /// The web backend in a real headless Chrome, on any host that has one.
    ///
    /// Fetches the ChromeDriver matching the installed Chrome from the Chrome
    /// for Testing archives into `target/chromedriver/` (once per Chrome
    /// version; `CHROMEDRIVER` names one to use instead, `FLUI_CHROME`
    /// another Chrome), then runs `flui-platform`'s `web_browser` test target
    /// in headless Chrome through `wasm-bindgen-test-runner`: the producer
    /// tests that dispatch DOM events at the real window's canvas through the
    /// public input callback.
    ///
    /// Then builds the `web_input_probe` example (the backend under a native
    /// capture, `native-capture`, debug), binds it with the locked
    /// `wasm-bindgen` into `target/web-input-probe/`, serves it on localhost
    /// and drives it over WebDriver BiDi with trusted input: a mouse click,
    /// double click, drag and secondary click, a pen stroke with pressure,
    /// angles and twist and two touches, whose recording must replay to what
    /// the window delivered and, natively, to the shape of its golden
    /// `web_bidi_mouse_pen_touch.jsonl` (`--bless` writes it instead); the
    /// Vibration API refused before the first click and played after it;
    /// an HTML drag the probe starts on a press, dropped on the page, and a
    /// page element dropped onto the canvas; and Chrome's own input-method
    /// path (`Input.imeSetComposition`, `Input.insertText` over ChromeDriver's
    /// DevTools channel) into a focused field through the `EditContext` and
    /// through the hidden textarea, with a trusted Escape during the
    /// composition. Exits 2 when the host has no Chrome.
    WebInput {
        /// Replace the golden recording with this run's instead of holding
        /// the run against it.
        #[arg(long)]
        bless: bool,
    },
    /// The interaction gallery in a real headless Chrome, every screen
    /// driven by trusted input and each outcome read through the gallery's
    /// semantics tree and the page's pixels, on any host that has Chrome.
    ///
    /// Fetches the ChromeDriver matching the installed Chrome (as
    /// `web-input` does), builds `examples/interaction_gallery` for
    /// wasm32 in release with its `agent` feature, binds it with the locked
    /// `wasm-bindgen` into `target/web-gallery/pkg/`, serves it on localhost
    /// and opens it at 1040 by 720 CSS pixels. Each screen is opened from
    /// the sidebar and driven over WebDriver BiDi with trusted mouse, wheel,
    /// key and touch input, one step per move, and with Chrome's input
    /// method over its DevTools path; the host's own mouse and keyboard are
    /// never used. The checks are the `windows-gallery` list with the
    /// page's input: swipe actions, pull to refresh (a pull, F5 and Ctrl+R,
    /// which must not reload the page), the collapsing header, the pager,
    /// the bottom sheet, zoom and pan (double tap and click, a spread,
    /// Ctrl+wheel, which must not zoom the page), drag and drop, the
    /// context menu (right click, long press, Shift+F10 and the menu key),
    /// the gesture pad and keyboard focus with an input-method composition.
    /// After each action it reads the outcome from the tree the page's
    /// agent serves (`globalThis.__flui_agent`) and from pixels of
    /// `browsingContext.captureScreenshot` sampled where the right and the
    /// wrong outcome differ. Every check is reported; the run fails if any
    /// did. Captures land in `target/web-gallery-shots/`.
    /// `FLUI_GALLERY_CHECKS=<text>` runs only the checks whose name contains
    /// `<text>` (`focus:`). Exits 2 when the host has no Chrome.
    WebGallery,
    /// The Android backend on an attached device, driven by Android's own
    /// injection, and the golden native recordings of what it made of it.
    ///
    /// Builds `flui-platform`'s `android_input_probe` (a `NativeActivity`
    /// library with `native-capture`) with `cargo ndk`, packages it into a
    /// debuggable APK with the SDK's build tools, installs it over `adb` and
    /// records three sessions, each in a fresh launch: `adb shell input`
    /// taps, swipes, a stylus and a mouse click; a `uinput` stylus that
    /// hovers, touches with pressure and tilt, lifts and leaves; and a
    /// `uinput` touchpad that pinches and scrolls with two fingers, which
    /// Android's gesture converter turns into its gesture events. Each
    /// recording is pulled into `target/native-recordings/android/` with the
    /// events the window delivered, and a host test replays it through the
    /// shared decoder: it must reproduce exactly what the device delivered,
    /// and the event grammar of its golden in
    /// `crates/flui-platform/tests/native_recording/` (with `--bless` the
    /// pulled recordings replace the goldens instead).
    ///
    /// A last launch checks the probe's left-edge system gesture exclusion
    /// in the window manager, the haptics switch and long-press timeout
    /// (changed with `settings put`, then put back) in `SystemPreferences`,
    /// and its haptics in the vibrator service's record: every vibration
    /// with the touch usage, a pattern played or fallen back as the device's
    /// vibrator allows. Needs the Android SDK (`ANDROID_HOME`), `cargo-ndk`,
    /// the `aarch64-linux-android` target, a JDK and one unlocked device
    /// (`FLUI_ANDROID_SERIAL` picks one of several); exits 2 without them.
    /// Do not touch the device while it runs.
    AndroidInput {
        /// Replace the golden recordings with this run's instead of holding
        /// the run against them.
        #[arg(long)]
        bless: bool,
    },
    /// The interaction gallery on an attached Android device, every screen
    /// driven by the system's own input and each outcome read through the
    /// gallery's development agent and the screen's pixels.
    ///
    /// Builds `examples/interaction_gallery`'s library with `agent` (debug,
    /// `cargo ndk`), packages it into a debuggable APK, installs it, writes
    /// the agent's launch token as the application's user (`run-as`) and
    /// launches it. FLUI publishes no Android accessibility tree, so the
    /// semantics tree is read from the development agent (ADR-0095 §3) over
    /// its socket, reached through `run-as … nc -U`. Each screen is opened
    /// from the tab strip and driven with `uinput` touches from a virtual
    /// multi-touch screen (taps, drags, flings, long presses, double taps,
    /// pinches), `input stylus` and `input mouse` events, hardware key
    /// events (`input keyevent`) and the device's own soft keyboard, then
    /// each outcome is read from the tree (the screen's readout, a row
    /// gone, a menu open, the focused element) and from `screencap` pixels
    /// where the right and the wrong outcome differ. Every check is
    /// reported; the run fails if any did. Captures land in
    /// `target/android-gallery/shots/`, and the gallery is uninstalled at
    /// the end. Needs the Android SDK, `cargo-ndk`, the
    /// `aarch64-linux-android` target, a JDK and one unlocked device
    /// (`FLUI_ANDROID_SERIAL` picks one); exits 2 without them. Do not
    /// touch the device while it runs. `FLUI_GALLERY_CHECKS=<text>` runs
    /// only the checks whose name contains `<text>`; `FLUI_GALLERY_KEEP=1`
    /// leaves the gallery installed for a look at what a failure left.
    AndroidGallery,
}

impl DeviceCheck {
    /// Whether this host is the OS the check needs.
    fn runs_on_this_host(&self) -> bool {
        if matches!(self, Self::WebInput { .. } | Self::WebGallery) {
            // Every host Chrome runs on; the first step says when it has none.
            return true;
        }
        if matches!(self, Self::AndroidInput { .. } | Self::AndroidGallery) {
            // Any host with the SDK drives a device over `adb`.
            return true;
        }
        let needs = if matches!(
            self,
            Self::WindowsA11y
                | Self::WindowsInput { .. }
                | Self::WindowsNotes
                | Self::WindowsGallery
        ) {
            "windows"
        } else {
            "macos"
        };
        std::env::consts::OS == needs
    }

    /// Why this check does nothing off its host, or `None` when its driver
    /// decides that itself.
    fn skip_reason(&self) -> Option<&'static str> {
        Some(match self {
            Self::MacosClosePath => {
                "Skipping macos-close-path on this host: the probe needs a real macOS host with an active GUI session and a staged .app bundle (AppKit window construction requires the main thread and a bundle); on a Mac run: cargo xtask device macos-close-path"
            }
            Self::MacosFramePump => {
                "Skipping macos-frame-pump on this host: the probe needs a real macOS host with an active GUI session and a staged .app bundle — it measures frames from a visible window, so it cannot run headless or on another OS; on a Mac run: cargo xtask device macos-frame-pump"
            }
            Self::MacosResizeJitter => {
                "Skipping macos-resize-jitter on this host: the probe measures the Metal swapchain of a real visible AppKit window under a resize burst, so it needs a real macOS host with an active GUI session and a staged .app bundle; on a Mac run: cargo xtask device macos-resize-jitter"
            }
            Self::MacosIme => {
                "Skipping macos-ime on this host: the probe needs a real macOS host with an active GUI session and a staged .app bundle — it routes AppKit key events into a visible window, so it cannot run headless or on another OS; on a Mac run: cargo xtask device macos-ime"
            }
            Self::MacosLaunchRender => {
                "Skipping macos-launch-render on this host: the gate photographs a real window on a real display, so it needs macOS with an active GUI session; on a Mac run: cargo xtask device macos-launch-render"
            }
            Self::MacosLifecycle => {
                "Skipping macos-lifecycle on this host: the probe drives AppKit window transitions on a real visible window, so it needs macOS with an active GUI session; on a Mac run: cargo xtask device macos-lifecycle"
            }
            Self::MacosA11y => {
                "Skipping macos-a11y on this host: the check reads a real window through the macOS accessibility API, so it needs macOS with an active GUI session; on a Mac run: cargo xtask device macos-a11y"
            }
            Self::IosSim => {
                "Skipping ios-sim on this host: it needs a macOS host with Xcode and the iOS Simulator (xcrun simctl) plus the aarch64-apple-ios-sim target; on a Mac run: cargo xtask device ios-sim"
            }
            Self::IosInputCheck { .. } => {
                "Skipping ios-input-check on this host: it needs a macOS host with Xcode, the aarch64-apple-ios-sim target and an already booted simulator; on a Mac run: cargo xtask device ios-input-check <udid>"
            }
            Self::IosInputCheckApp { .. } => {
                "Skipping ios-input-check-app on this host: it needs a macOS host with Xcode, the aarch64-apple-ios-sim target and an already booted simulator; on a Mac run: cargo xtask device ios-input-check-app <app> <udid>"
            }
            Self::IosSafeAreaCheck { .. } => {
                "Skipping ios-safe-area-check on this host: it needs a macOS host with Xcode, the aarch64-apple-ios-sim target and an already booted simulator; on a Mac run: cargo xtask device ios-safe-area-check <udid>"
            }
            Self::WindowsA11y => {
                "Skipping windows-a11y on this host: the check reads a real window through UI Automation, so it needs Windows with an interactive desktop; on Windows run: cargo xtask device windows-a11y"
            }
            Self::WindowsInput { .. } => {
                "Skipping windows-input on this host: the check sends real pointer and keyboard input to a real window, so it needs Windows with an interactive desktop; on Windows run: cargo xtask device windows-input"
            }
            Self::WindowsNotes => {
                "Skipping windows-notes on this host: it needs Windows with an interactive desktop; on Windows run: cargo xtask device windows-notes"
            }
            Self::WindowsGallery => {
                "Skipping windows-gallery on this host: it drives a real window with real input, so it needs Windows with an interactive desktop; on Windows run: cargo xtask device windows-gallery"
            }
            Self::MacosWorkload
            | Self::MacosHotReloadLoop { .. }
            | Self::WebInput { .. }
            | Self::WebGallery
            | Self::AndroidInput { .. }
            | Self::AndroidGallery => {
                return None;
            }
        })
    }

    /// The steps this check takes on a host that runs it.
    fn plan(&self, context: &Context) -> Vec<Step> {
        let target = context.target_dir.as_path();
        match self {
            Self::MacosClosePath => bundled_probe(&CLOSE_PATH, target),
            Self::MacosFramePump => bundled_probe(&FRAME_PUMP, target),
            Self::MacosResizeJitter => bundled_probe(&RESIZE_JITTER, target),
            Self::MacosIme => bundled_probe(&IME, target),
            Self::MacosLaunchRender => launch_render(target),
            Self::MacosLifecycle => lifecycle(target),
            Self::MacosA11y => vec![Step::Driver {
                run: python_script("tools/device-checks/check-macos-a11y.py"),
                announce: Some(Announce {
                    cannot_verify: "macos-a11y CANNOT VERIFY: this host could not take the measurement (accessibility trust not granted, or swiftc missing) — details above",
                    failed: "macos-a11y FAILED: the button was not in the accessibility tree, AXPress was refused, or the count did not advance (tree dumps above)",
                }),
            }],
            Self::MacosWorkload => vec![Step::Driver {
                run: python_script("tools/device-checks/check-macos-workload.py"),
                announce: None,
            }],
            Self::MacosHotReloadLoop { work } => hot_reload_loop(work, target),
            Self::IosSim => ios_sim(&context.sim_device, target),
            Self::IosInputCheck { udid } => ios_input_check(udid, target),
            Self::IosInputCheckApp { app, udid, args } => vec![Step::Driver {
                run: python_script(INPUT_DRIVER).arg(udid).arg(app).args(args),
                announce: None,
            }],
            Self::IosSafeAreaCheck { udid } => vec![Step::Driver {
                run: python_script("tools/device-checks/check-ios-safe-area.py")
                    .args([udid.as_str(), "target/ios-safe-area-check"]),
                announce: None,
            }],
            Self::WindowsA11y => vec![
                build_a11y_probe(),
                Step::Native {
                    check: Native::WindowsA11y {
                        probe: target.join(A11Y_PROBE_EXE),
                    },
                    announce: Announce {
                        cannot_verify: "windows-a11y CANNOT VERIFY: input does not reach this desktop (locked or headless), or UI Automation could not be instantiated on this host — details above",
                        failed: "windows-a11y FAILED: the native tree, Invoke, disclosure transition, numeric range or role-family contract failed (tree dumps above)",
                    },
                },
            ],
            Self::WindowsNotes => vec![
                cargo_build([
                    "-p",
                    "flui",
                    "--locked",
                    "--release",
                    "--example",
                    "two_screens",
                    "--features",
                    "material,a11y",
                ]),
                Step::Native {
                    check: Native::WindowsNotes {
                        probe: target.join("release/examples/two_screens.exe"),
                    },
                    announce: Announce {
                        cannot_verify: "windows-notes CANNOT VERIFY: COM, DPI, foreground or native input was refused (details above)",
                        failed: "windows-notes FAILED: a Notes workflow observation or graceful close failed (tree dump above)",
                    },
                },
            ],
            Self::WindowsGallery => vec![
                cargo_build([
                    "-p",
                    "flui-interaction-gallery",
                    "--locked",
                    "--release",
                    "--features",
                    "a11y",
                ]),
                Step::Native {
                    check: Native::WindowsGallery {
                        gallery: target.join("release/interaction_gallery.exe"),
                        shots: target.join("interaction-gallery"),
                    },
                    announce: Announce {
                        cannot_verify: "windows-gallery CANNOT VERIFY: the foreground, the input or UI Automation was refused (details above)",
                        failed: "windows-gallery FAILED: a screen's outcome did not reach the tree or the pixels (summary above)",
                    },
                },
            ],
            Self::WebInput { bless } => web_input_plan(target, *bless),
            Self::WebGallery => web_gallery_plan(target),
            Self::WindowsInput { bless } => INPUT_SESSIONS
                .into_iter()
                .map(|session| session.record(target, *bless))
                .chain([build_a11y_probe()])
                .chain([Step::Native {
                    check: Native::WindowsInput {
                        probe: target.join(A11Y_PROBE_EXE),
                    },
                    announce: Announce {
                        cannot_verify: "windows-input CANNOT VERIFY: the probe window could not be kept in the foreground, or UI Automation was unavailable — no input was sent past that point; details above",
                        failed: "windows-input FAILED: a missed click changed the count, or a click on the button or Tab then Enter did not advance it (tree dumps above)",
                    },
                }])
                .collect(),
            Self::AndroidInput { bless } => android_input(target, *bless),
            Self::AndroidGallery => vec![
                Step::Run(Run::new(Program::Cargo).args([
                    "ndk",
                    "-t",
                    "arm64-v8a",
                    "build",
                    "-p",
                    "flui-interaction-gallery",
                    "--locked",
                    "--lib",
                    "--features",
                    "agent",
                ])),
                Step::Native {
                    check: Native::AndroidGallery {
                        library: target
                            .join("aarch64-linux-android/debug/libflui_interaction_gallery.so"),
                        work: target.join("android-gallery"),
                        shots: target.join("android-gallery/shots"),
                    },
                    announce: Announce {
                        cannot_verify: "android-gallery CANNOT VERIFY: no Android SDK, no attached device, a locked device, or no agent answered (details above)",
                        failed: "android-gallery FAILED: a screen's outcome did not reach the tree or the pixels (summary above)",
                    },
                },
            ],
        }
    }
}

/// Where `android-input` pulls each session's recording.
const ANDROID_RECORDINGS: &str = "native-recordings/android";

/// Build the probe, drive it on the device, then hold what it pulled.
fn android_input(target: &Path, bless: bool) -> Vec<Step> {
    let recordings = target.join(ANDROID_RECORDINGS);
    let mut hold = Run::new(Program::Cargo)
        .args([
            "test",
            "-p",
            "flui-platform",
            "--locked",
            "--test",
            "platform_it",
            "--features",
            "native-capture",
            "--",
            "--exact",
            "native_recording::pulled_device_recordings_replay_to_what_they_delivered",
            "--nocapture",
        ])
        .env(
            "FLUI_DEVICE_RECORDING_DIR",
            format!("{}/{ANDROID_RECORDINGS}", target.display()),
        );
    if bless {
        hold = hold.env("FLUI_NATIVE_RECORDING_BLESS", "1");
    }
    vec![
        Step::Run(Run::new(Program::Cargo).args([
            "ndk",
            "-t",
            "arm64-v8a",
            "build",
            "-p",
            "flui-platform",
            "--locked",
            "--example",
            "android_input_probe",
            "--features",
            "native-capture",
        ])),
        Step::RemoveDir(recordings.clone()),
        Step::Native {
            check: Native::AndroidInput {
                probe: target
                    .join("aarch64-linux-android/debug/examples/libandroid_input_probe.so"),
                recordings,
                work: target.join("android-input"),
            },
            announce: Announce {
                cannot_verify: "android-input CANNOT VERIFY: no Android SDK, no attached device, or the device could not show the probe (locked) — details above",
                failed: "android-input FAILED: a session was not flushed, the gesture exclusion, the preferences or the haptics did not reach the system, or FluiView did not carry the drag or the keyboard (details above)",
            },
        },
        Step::Probe {
            run: hold,
            markers: &["DEVICE_RECORDINGS_HELD=3"],
            failure: "android-input FAILED: a pulled recording did not replay to what the device delivered, or no longer matches its golden (output above)",
        },
    ]
}

/// An injected input session of `flui-platform`'s native window contract,
/// recorded under `native-capture` and held against its golden.
#[derive(Debug, Clone, Copy)]
struct InputSession {
    /// The contract's child row that injects and records it.
    child: &'static str,
    /// What the row prints once its recording was held against, or written
    /// as, its golden; a row that cannot run on this desktop prints
    /// `CANNOT_VERIFY` and not this.
    held: &'static [&'static str],
}

/// The sessions `windows-input` records, in order.
const INPUT_SESSIONS: [InputSession; 2] = [
    InputSession {
        child: "a_captured_native_pointer_session_replays_to_what_it_delivered",
        held: &["NATIVE_RECORDING_HELD=win32_injected_touch_and_pen"],
    },
    InputSession {
        child: "a_captured_native_mouse_session_replays_to_what_it_delivered",
        held: &["NATIVE_RECORDING_HELD=win32_sent_mouse_and_wheel"],
    },
];

impl InputSession {
    /// The step that runs this session in its own test process, writing its
    /// fresh recording under `target` and, with `bless`, over its golden.
    fn record(self, target: &Path, bless: bool) -> Step {
        let mut run = Run::new(Program::Cargo)
            .args([
                "test",
                "-p",
                "flui-platform",
                "--locked",
                "--test",
                "platform_it",
                "--features",
                "native-capture",
                "--",
                "--exact",
                "contract::test_window_lifecycle_contract",
                "--nocapture",
            ])
            .env("FLUI_NATIVE_WINDOW_CONTRACT_CHILD", self.child)
            .env(
                "FLUI_NATIVE_RECORDING_DIR",
                format!("{}/native-recordings", target.display()),
            );
        if bless {
            run = run.env("FLUI_NATIVE_RECORDING_BLESS", "1");
        }
        Step::Probe {
            run,
            markers: self.held,
            failure: "windows-input FAILED or CANNOT VERIFY: an injected session did not replay to what the window delivered, no longer matches its golden, or could not run on this desktop (CANNOT_VERIFY, output above)",
        }
    }
}

/// The ChromeDriver every `web-input` step after the first names.
fn chromedriver_at(target: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "chromedriver.exe"
    } else {
        "chromedriver"
    };
    target.join("chromedriver").join("current").join(name)
}

/// `web-input`: the ChromeDriver, the browser test target in headless
/// Chrome, then the probe page driven over WebDriver BiDi and its recording
/// held against its golden (or, with `bless`, written over it).
fn web_input_plan(target: &Path, bless: bool) -> Vec<Step> {
    let driver = chromedriver_at(target);
    let page = target.join("web-input-probe");
    let recordings = target.join("native-recordings");
    let mut hold = Run::new(Program::Cargo)
        .args([
            "test",
            "-p",
            "flui-platform",
            "--locked",
            "--test",
            "platform_it",
            "--features",
            "native-capture",
            "--",
            "--exact",
            "native_recording::browser_sessions_hold_against_their_goldens",
            "--nocapture",
        ])
        .env(
            "FLUI_NATIVE_RECORDING_FRESH",
            recordings.display().to_string(),
        );
    if bless {
        hold = hold.env("FLUI_NATIVE_RECORDING_BLESS", "1");
    }
    vec![
        Step::Native {
            check: Native::Chromedriver {
                cache: target.join("chromedriver"),
                driver: driver.clone(),
            },
            announce: Announce {
                cannot_verify: "web-input CANNOT VERIFY: this host has no Chrome (details above)",
                failed: "web-input FAILED: the ChromeDriver for this Chrome could not be fetched (details above)",
            },
        },
        Step::Run(
            Run::new(Program::Cargo)
                .args([
                    "test",
                    "-p",
                    "flui-platform",
                    "--locked",
                    "--target",
                    "wasm32-unknown-unknown",
                    "--test",
                    "web_browser",
                ])
                .env("CHROMEDRIVER", driver.display().to_string()),
        ),
        cargo_build([
            "-p",
            "flui-platform",
            "--locked",
            "--example",
            "web_input_probe",
            "--features",
            "native-capture",
            "--target",
            "wasm32-unknown-unknown",
        ]),
        Step::RemoveDir(page.clone()),
        Step::Run(
            Run::new(Program::Tool("wasm-bindgen"))
                .args(["--target", "web", "--out-dir"])
                .arg(page.clone())
                .arg(target.join("wasm32-unknown-unknown/debug/examples/web_input_probe.wasm")),
        ),
        Step::Copy {
            from: PathBuf::from("crates/flui-platform/examples/web_input_probe/index.html"),
            to: page.join("index.html"),
        },
        Step::Native {
            check: Native::WebInput {
                page,
                driver,
                recordings,
            },
            announce: Announce {
                cannot_verify: "web-input CANNOT VERIFY: the probe page could not be driven (details above)",
                failed: "web-input FAILED: a trusted-input check of the probe page failed (verdicts above)",
            },
        },
        Step::Probe {
            run: hold,
            markers: &["NATIVE_RECORDING_HELD=web_bidi_mouse_pen_touch"],
            failure: "web-input FAILED: the browser session no longer replays to its golden (output above)",
        },
    ]
}

/// `web-gallery`: the ChromeDriver, the gallery built for the page with its
/// driver agent and bound, then every screen driven over WebDriver BiDi.
fn web_gallery_plan(target: &Path) -> Vec<Step> {
    let driver = chromedriver_at(target);
    let page = target.join("web-gallery");
    vec![
        Step::Native {
            check: Native::Chromedriver {
                cache: target.join("chromedriver"),
                driver: driver.clone(),
            },
            announce: Announce {
                cannot_verify: "web-gallery CANNOT VERIFY: this host has no Chrome (details above)",
                failed: "web-gallery FAILED: the ChromeDriver for this Chrome could not be fetched (details above)",
            },
        },
        cargo_build([
            "-p",
            "flui-interaction-gallery",
            "--locked",
            "--release",
            "--lib",
            "--features",
            "agent",
            "--target",
            "wasm32-unknown-unknown",
        ]),
        Step::RemoveDir(page.clone()),
        Step::Run(
            Run::new(Program::Tool("wasm-bindgen"))
                .args(["--target", "web", "--out-dir"])
                .arg(page.join("pkg"))
                .arg(target.join("wasm32-unknown-unknown/release/flui_interaction_gallery.wasm")),
        ),
        Step::Copy {
            from: PathBuf::from("examples/interaction_gallery/index.html"),
            to: page.join("index.html"),
        },
        Step::Native {
            check: Native::WebGallery {
                page,
                driver,
                shots: target.join("web-gallery-shots"),
            },
            announce: Announce {
                cannot_verify: "web-gallery CANNOT VERIFY: the gallery page could not be driven (details above)",
                failed: "web-gallery FAILED: a screen's outcome did not reach the tree or the pixels (summary above)",
            },
        },
    ]
}

/// What the plans take from the host.
#[derive(Debug)]
struct Context {
    /// Cargo's target directory as `cargo metadata` reports it, so
    /// `CARGO_TARGET_DIR` and `build.target-dir` are honoured.
    target_dir: PathBuf,
    /// The simulator `ios-sim` boots.
    sim_device: String,
}

impl Context {
    fn from_host(root: &Path) -> anyhow::Result<Self> {
        let target_dir = metadata(root)?.target_directory.into_std_path_buf();
        let sim_device = std::env::var("FLUI_IOS_SIM_DEVICE")
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| DEFAULT_SIM_DEVICE.to_owned());
        Ok(Self {
            target_dir,
            sim_device,
        })
    }
}

/// Where the release build of `a11y_probe` lands on Windows.
const A11Y_PROBE_EXE: &str = "release/examples/a11y_probe.exe";

/// The counter with the AccessKit adapter installed, which both Windows
/// checks run.
fn build_a11y_probe() -> Step {
    cargo_build([
        "-p",
        "flui",
        "--locked",
        "--release",
        "--example",
        "a11y_probe",
        "--features",
        "material,a11y",
    ])
}

fn cargo_build<const N: usize>(args: [&str; N]) -> Step {
    Step::Run(Run::new(Program::Cargo).arg("build").args(args))
}

fn xcrun<const N: usize>(args: [&str; N]) -> Run {
    Run::new(Program::Tool("xcrun")).args(args)
}

/// A driver under `-B`, so no `__pycache__` lands in the tree.
fn python_script(script: &str) -> Run {
    Run::new(Program::Python).args(["-B", script])
}

/// An AppKit probe that has to run from inside a `.app`: AppKit builds real
/// windows only on the main thread of a bundled process, which unbundled
/// libtest cannot provide.
struct BundledProbe {
    package: &'static str,
    example: &'static str,
    /// The staged bundle.
    app: &'static str,
    /// The committed Info.plist the bundle carries.
    info_plist: &'static str,
    markers: &'static [&'static str],
    failure: &'static str,
}

const CLOSE_PATH: BundledProbe = BundledProbe {
    package: "flui-platform",
    example: "close_path_probe",
    app: "target/macos-close-path/ClosePathProbe.app",
    info_plist: "crates/flui-platform/examples/Info.plist.close_path_probe",
    markers: &["CLOSE_PATH_PROBE_RESULT=PASS"],
    failure: "macos-close-path FAILED: probe exit code or PASS marker missing (output above)",
};

const FRAME_PUMP: BundledProbe = BundledProbe {
    package: "flui-platform",
    example: "frame_pump_probe",
    app: "target/macos-frame-pump/FramePumpProbe.app",
    info_plist: "crates/flui-platform/examples/Info.plist.frame_pump_probe",
    markers: &["FRAME_PUMP_PROBE_RESULT=PASS"],
    failure: "macos-frame-pump FAILED: probe exit code or PASS marker missing (output above)",
};

const RESIZE_JITTER: BundledProbe = BundledProbe {
    package: "flui",
    example: "resize_jitter_probe",
    app: "target/macos-resize-jitter/ResizeJitterProbe.app",
    info_plist: "examples/Info.plist.resize_jitter_probe",
    markers: &[
        "RESIZE_JITTER_PROBE_RESULT=PASS",
        "RESIZE_JITTER_PROBE_STALE=0",
    ],
    failure: "macos-resize-jitter FAILED: probe exit code, PASS marker, or the zero stale-size marker is missing (output above)",
};

const IME: BundledProbe = BundledProbe {
    package: "flui-platform",
    example: "ime_probe",
    app: "target/macos-ime/ImeProbe.app",
    info_plist: "crates/flui-platform/examples/Info.plist.ime_probe",
    markers: &["IME_PROBE_RESULT=PASS"],
    failure: "macos-ime FAILED: probe exit code or PASS marker missing (output above)",
};

fn bundled_probe(probe: &BundledProbe, target: &Path) -> Vec<Step> {
    let app = Path::new(probe.app);
    let binary = app.join("Contents/MacOS").join(probe.example);
    vec![
        cargo_build(["-p", probe.package, "--locked", "--example", probe.example]),
        Step::RemoveDir(app.to_path_buf()),
        Step::CreateDir(app.join("Contents/MacOS")),
        Step::Copy {
            from: probe.info_plist.into(),
            to: app.join("Contents/Info.plist"),
        },
        Step::Copy {
            from: target.join("debug/examples").join(probe.example),
            to: binary.clone(),
        },
        Step::Probe {
            run: Run::new(Program::Path(binary)).env("RUST_LOG", "info"),
            markers: probe.markers,
            failure: probe.failure,
        },
    ]
}

fn launch_render(target: &Path) -> Vec<Step> {
    vec![
        cargo_build(["-p", "flui", "--locked", "--example", "colored_box_app"]),
        Step::Driver {
            run: Run::new(Program::Python)
                .arg("tools/device-checks/check-macos-launch-render.py")
                .arg(target.join("debug/examples/colored_box_app"))
                .args(["--runs", "5", "--expect", "240,0,0"]),
            announce: Some(Announce {
                cannot_verify: "macos-launch-render CANNOT VERIFY: this host could not take the measurement (Screen Recording not granted, or swiftc missing) — a denied capture is NOT a blank window, so nothing was decided; details above",
                failed: "macos-launch-render FAILED: a launch route was refused, put no window on screen, or put up a window that stayed blank for the whole settle - details and images above",
            }),
        },
    ]
}

fn lifecycle(target: &Path) -> Vec<Step> {
    vec![
        cargo_build([
            "-p",
            "flui",
            "--locked",
            "--release",
            "--example",
            "lifecycle_probe",
            "--features",
            "material",
        ]),
        Step::Probe {
            run: Run::new(Program::Path(
                target.join("release/examples/lifecycle_probe"),
            ))
            .env("RUST_LOG", "warn"),
            markers: &["LIFECYCLE_PROBE_RESULT=PASS"],
            failure: "macos-lifecycle FAILED: a phase was over budget or the probe did not finish (phase lines above)",
        },
    ]
}

/// The CLI is named explicitly: the driver's own default is
/// `<root>/target/debug/flui`, which is not where the build above put it
/// when the target directory is elsewhere.
fn hot_reload_loop(work: &Path, target: &Path) -> Vec<Step> {
    vec![
        cargo_build(["-p", "flui-cli", "--locked"]),
        Step::Driver {
            run: python_script("tools/device-checks/check-hot-reload-loop.py")
                .arg(work)
                .arg("--cli")
                .arg(target.join("debug/flui")),
            announce: None,
        },
    ]
}

/// The Material demo for the simulator, which `ios-sim` and
/// `ios-input-check` both stage.
fn build_ios_demo() -> Step {
    cargo_build([
        "-p",
        "flui",
        "--locked",
        "--features",
        "material",
        "--example",
        "ios_demo",
        "--target",
        IOS_SIM_TARGET,
    ])
}

/// Stages a simulator build into a flat iOS `.app`: Info.plist and the binary
/// side by side at the top.
fn stage_ios_app(app: &str, info_plist: &str, target: &Path, example: &str) -> [Step; 4] {
    let app = Path::new(app);
    [
        Step::RemoveDir(app.to_path_buf()),
        Step::CreateDir(app.to_path_buf()),
        Step::Copy {
            from: info_plist.into(),
            to: app.join("Info.plist"),
        },
        Step::Copy {
            from: target
                .join(IOS_SIM_TARGET)
                .join("debug/examples")
                .join(example),
            to: app.join(example),
        },
    ]
}

/// Two arms on one booted simulator: a static app that must create a Metal
/// device and render a frame, then an animated app whose pixels must move.
fn ios_sim(device: &str, target: &Path) -> Vec<Step> {
    const APP: &str = "target/ios-sim/IosDemo.app";
    const LOG: &str = "target/ios-sim/app.log";
    const ANIM_BUNDLE: &str = "dev.flui.anim-demo";
    const ANIM_APP: &str = "target/ios-sim/AnimDemo.app";
    const ANIM_A: &str = "target/ios-sim/anim_a.png";
    const ANIM_B: &str = "target/ios-sim/anim_b.png";
    const CROP_A: &str = "target/ios-sim/anim_a_crop.png";
    const CROP_B: &str = "target/ios-sim/anim_b_crop.png";

    let mut steps = vec![
        Step::Run(xcrun(["simctl", "boot", device]).quiet_stderr().or_true()),
        Step::Run(
            xcrun(["simctl", "bootstatus", device, "-b"])
                .silent()
                .or_true(),
        ),
        // Arm 1 — a static Material app: Metal device created and a frame
        // rendered, read out of the unified log.
        build_ios_demo(),
    ];
    steps.extend(stage_ios_app(
        APP,
        "examples/Info.plist.ios_demo",
        target,
        "ios_demo",
    ));
    steps.extend([
        Step::Run(xcrun(["simctl", "install", "booted", APP])),
        Step::Run(
            xcrun(["simctl", "terminate", "booted", IOS_DEMO_BUNDLE])
                .quiet_stderr()
                .or_true(),
        ),
        Step::Run(xcrun(["simctl", "launch", "booted", IOS_DEMO_BUNDLE]).stdout(Sink::Null)),
        Step::Sleep(Duration::from_secs(12)),
        Step::Run(
            xcrun([
                "simctl",
                "spawn",
                "booted",
                "log",
                "show",
                "--last",
                "5m",
                "--process",
                "ios_demo",
            ])
            .stdout(Sink::File(LOG.into()))
            .quiet_stderr()
            .or_true(),
        ),
        Step::Run(
            xcrun(["simctl", "io", "booted", "screenshot", "target/ios-sim/screen.png"])
                .silent()
                .or_true(),
        ),
        Step::RequireLog {
            log: LOG.into(),
            patterns: &["Selected GPU:.*Metal", "First frame rendered"],
            failure: "IOS_SIM_RESULT=FAIL - arm 1 (static app): expected \"Selected GPU ... Metal\" and \"First frame rendered\"; tail:",
            excerpt: "flui]",
            tail: 20,
        },
        Step::Echo("IOS_SIM arm1=PASS (Metal device created, first frame rendered)"),
    ]);

    // Arm 2 — an animated app, and the regression guard for the iOS
    // frame-source bug. Survival alone is not the discriminator: a
    // request_redraw that dispatches a frame synchronously and calls
    // setNeedsDisplay() can still complete and log frames while the screen
    // stays white, because UIKit repaints the opaque UIView's empty layer over
    // the CAMetalLayer the renderer presented into. The honest signal is the
    // pixels: two screenshots of a live, animating tree must differ. A
    // cropped centre square is compared so a ticking status-bar clock can
    // never masquerade as motion.
    steps.push(cargo_build([
        "-p",
        "flui",
        "--locked",
        "--example",
        "animated_box_app",
        "--target",
        IOS_SIM_TARGET,
    ]));
    steps.extend(stage_ios_app(
        ANIM_APP,
        "examples/Info.plist.ios_anim",
        target,
        "animated_box_app",
    ));
    let screenshot = |path: &str| {
        Step::Run(
            xcrun(["simctl", "io", "booted", "screenshot", path])
                .silent()
                .or_true(),
        )
    };
    let crop = |path: &str| {
        Step::Run(
            Run::new(Program::Tool("sips"))
                .args(["-c", "240", "240", path])
                .silent()
                .or_true(),
        )
    };
    let terminate = || {
        Step::Run(
            xcrun(["simctl", "terminate", "booted", ANIM_BUNDLE])
                .quiet_stderr()
                .or_true(),
        )
    };
    steps.extend([
        Step::Run(xcrun(["simctl", "install", "booted", ANIM_APP])),
        terminate(),
        Step::Run(xcrun(["simctl", "launch", "booted", ANIM_BUNDLE]).stdout(Sink::Null)),
        Step::Sleep(Duration::from_secs(12)),
        screenshot(ANIM_A),
        Step::Sleep(Duration::from_secs(2)),
        screenshot(ANIM_B),
        Step::Copy {
            from: ANIM_A.into(),
            to: CROP_A.into(),
        },
        Step::Copy {
            from: ANIM_B.into(),
            to: CROP_B.into(),
        },
        crop(CROP_A),
        crop(CROP_B),
        // Survival is counted before the app is terminated and judged after;
        // the crops cannot change in between, so comparing them after the
        // terminate is the same verdict.
        Step::CountProcesses("animated_box_app"),
        terminate(),
        Step::RequireDiffer {
            a: CROP_A.into(),
            b: CROP_B.into(),
            failure: "IOS_SIM_RESULT=FAIL - arm 2 (animated app): two screenshots 2 s apart are IDENTICAL, so no animation reached the screen (frozen or white) even though frames may be logged; see target/ios-sim/anim_{a,b}.png",
        },
        Step::RequireSurvivor(
            "IOS_SIM_RESULT=FAIL - arm 2 (animated app): the app did not survive to the screenshot pass - request_redraw likely never returned to UIKit and iOS scene-create watchdog killed it",
        ),
        Step::Echo("IOS_SIM arm2=PASS (animated pixels differ, app survived)"),
        Step::Echo("IOS_SIM_RESULT=PASS (static + animated, screenshots under target/ios-sim/)"),
    ]);
    steps
}

fn ios_input_check(udid: &str, target: &Path) -> Vec<Step> {
    const APP: &str = "target/ios-input/IosDemo.app";
    let mut steps = vec![build_ios_demo()];
    steps.extend(stage_ios_app(
        APP,
        "examples/Info.plist.ios_demo",
        target,
        "ios_demo",
    ));
    // Remove any previous install before the run. xcodebuild installs the
    // artifact it was pointed at, and XCUIApplication launches whatever is
    // installed under that bundle id, so a stale copy left here could be the
    // binary actually measured. With it gone, a failed install means nothing
    // launches and the probe reports CANNOT VERIFY instead of testing the
    // wrong build.
    steps.push(Step::Run(
        xcrun(["simctl", "uninstall", udid, IOS_DEMO_BUNDLE])
            .quiet_stderr()
            .or_true(),
    ));
    steps.push(Step::Driver {
        run: python_script(INPUT_DRIVER).args([udid, APP]),
        announce: Some(Announce {
            cannot_verify: "ios-input-check CANNOT VERIFY: this host could not take the measurement (no Xcode toolchain, the simulator was not booted, or the probe produced no report) - nothing was decided about the framework; details above",
            failed: "ios-input-check FAILED: a real touch did not reach a widget, or the state it changed did not survive Home/return, or a control did not behave - details and per-stage screenshots above",
        }),
    });
    steps
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory as _, Parser as _};

    use super::*;

    #[derive(Debug, clap::Parser)]
    struct Cli {
        #[command(subcommand)]
        check: DeviceCheck,
    }

    fn parse(args: &[&str]) -> Result<DeviceCheck, clap::Error> {
        Cli::try_parse_from(std::iter::once("device").chain(args.iter().copied()))
            .map(|cli| cli.check)
    }

    /// The target directory the recipes used when `CARGO_TARGET_DIR` was
    /// unset, so a plan renders exactly as its recipe expanded.
    fn context() -> Context {
        Context {
            target_dir: "target".into(),
            sim_device: DEFAULT_SIM_DEVICE.to_owned(),
        }
    }

    fn render(check: &DeviceCheck) -> Vec<String> {
        check
            .plan(&context())
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn assert_plan(check: &DeviceCheck, recipe: &[&str]) {
        let plan = render(check);
        for (index, (step, line)) in plan.iter().zip(recipe).enumerate() {
            assert_eq!(step, line, "step {index} of {check:?}");
        }
        assert_eq!(plan.len(), recipe.len(), "{check:?}: {plan:#?}");
    }

    fn the_cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    fn every_recipe_name_parses_to_its_check() {
        for (args, check) in [
            (&["macos-close-path"][..], DeviceCheck::MacosClosePath),
            (&["macos-frame-pump"], DeviceCheck::MacosFramePump),
            (&["macos-resize-jitter"], DeviceCheck::MacosResizeJitter),
            (&["macos-ime"], DeviceCheck::MacosIme),
            (&["macos-launch-render"], DeviceCheck::MacosLaunchRender),
            (&["macos-lifecycle"], DeviceCheck::MacosLifecycle),
            (&["macos-a11y"], DeviceCheck::MacosA11y),
            (&["macos-workload"], DeviceCheck::MacosWorkload),
            (
                &["macos-hot-reload-loop"],
                DeviceCheck::MacosHotReloadLoop {
                    work: "target/hot-reload-loop/work".into(),
                },
            ),
            (
                &["macos-hot-reload-loop", "scratch"],
                DeviceCheck::MacosHotReloadLoop {
                    work: "scratch".into(),
                },
            ),
            (&["ios-sim"], DeviceCheck::IosSim),
            (&["windows-a11y"], DeviceCheck::WindowsA11y),
            (&["web-input"], DeviceCheck::WebInput { bless: false }),
            (
                &["web-input", "--bless"],
                DeviceCheck::WebInput { bless: true },
            ),
            (
                &["windows-input"],
                DeviceCheck::WindowsInput { bless: false },
            ),
            (
                &["windows-input", "--bless"],
                DeviceCheck::WindowsInput { bless: true },
            ),
            (&["windows-notes"], DeviceCheck::WindowsNotes),
            (&["windows-gallery"], DeviceCheck::WindowsGallery),
            (&["web-gallery"], DeviceCheck::WebGallery),
            (&["android-gallery"], DeviceCheck::AndroidGallery),
            (
                &["android-input"],
                DeviceCheck::AndroidInput { bless: false },
            ),
            (
                &["android-input", "--bless"],
                DeviceCheck::AndroidInput { bless: true },
            ),
            (
                &["ios-input-check", "UDID-1"],
                DeviceCheck::IosInputCheck {
                    udid: "UDID-1".into(),
                },
            ),
            (
                &["ios-safe-area-check", "UDID-1"],
                DeviceCheck::IosSafeAreaCheck {
                    udid: "UDID-1".into(),
                },
            ),
        ] {
            assert_eq!(parse(args).expect("parses"), check, "{args:?}");
        }
    }

    /// Runs on any host: the probe is built for the device, the stale pull
    /// removed, the device driven, and only then is the pull held against
    /// the goldens; `--bless` writes them instead. A device step that fails
    /// ends the plan before the hold.
    fn android_input_drives_the_device_then_holds_what_it_pulled() {
        assert!(DeviceCheck::AndroidInput { bless: false }.runs_on_this_host());
        for (bless, flag) in [(false, ""), (true, " FLUI_NATIVE_RECORDING_BLESS=1")] {
            let hold = format!(
                "out=$(FLUI_DEVICE_RECORDING_DIR=target/native-recordings/android{flag} cargo test -p flui-platform --locked --test platform_it --features native-capture -- --exact native_recording::pulled_device_recordings_replay_to_what_they_delivered --nocapture 2>&1); printf '%s\\n' \"$out\"; unless exit 0 and 'DEVICE_RECORDINGS_HELD=3' in $out: echo 'android-input FAILED: a pulled recording did not replay to what the device delivered, or no longer matches its golden (output above)'; exit 1"
            );
            assert_plan(
                &DeviceCheck::AndroidInput { bless },
                &[
                    "cargo ndk -t arm64-v8a build -p flui-platform --locked --example android_input_probe --features native-capture",
                    "rm -rf target/native-recordings/android",
                    "adb-session target/aarch64-linux-android/debug/examples/libandroid_input_probe.so target/native-recordings/android target/android-input; if rc=2: echo 'android-input CANNOT VERIFY: no Android SDK, no attached device, or the device could not show the probe (locked) — details above'; elif rc!=0: echo 'android-input FAILED: a session was not flushed, the gesture exclusion, the preferences or the haptics did not reach the system, or FluiView did not carry the drag or the keyboard (details above)'; if rc!=0: exit $rc",
                    &hold,
                ],
            );
        }
    }

    fn the_input_check_passes_its_driver_arguments_through_verbatim() {
        let check = parse(&[
            "ios-input-check-app",
            "build/Counter.app",
            "UDID-1",
            "--target-tap",
            "0.5,0.097",
            "--empty-tap",
            "0.5,0.5",
            "--region",
            "0.25,0.0,0.75,0.96",
            "--post-return-tap",
        ])
        .expect("parses");
        assert_plan(
            &check,
            &[
                "python -B tools/device-checks/check-ios-input.py UDID-1 build/Counter.app --target-tap 0.5,0.097 --empty-tap 0.5,0.5 --region 0.25,0.0,0.75,0.96 --post-return-tap; exit $rc",
            ],
        );
        // `+ARGS`: at least one driver argument, as the recipe required.
        assert!(parse(&["ios-input-check-app", "build/Counter.app", "UDID-1"]).is_err());
        assert!(parse(&["ios-input-check", "UDID-1", "extra"]).is_err());
        assert!(parse(&["ios-input-check"]).is_err());
    }

    fn off_macos_the_gated_checks_skip_and_the_drivers_decide_the_rest() {
        let gated = [
            DeviceCheck::MacosClosePath,
            DeviceCheck::MacosFramePump,
            DeviceCheck::MacosResizeJitter,
            DeviceCheck::MacosIme,
            DeviceCheck::MacosLaunchRender,
            DeviceCheck::MacosLifecycle,
            DeviceCheck::MacosA11y,
            DeviceCheck::IosSim,
            DeviceCheck::IosInputCheck { udid: "U".into() },
            DeviceCheck::IosInputCheckApp {
                app: "A.app".into(),
                udid: "U".into(),
                args: vec!["--post-return-tap".into()],
            },
            DeviceCheck::IosSafeAreaCheck { udid: "U".into() },
        ];
        for check in gated {
            let reason = check.skip_reason().expect("gated");
            let name = reason
                .strip_prefix("Skipping ")
                .and_then(|rest| rest.split_once(' '))
                .map(|(name, _)| name)
                .expect("names the check");
            assert!(
                reason.contains(&format!("; on a Mac run: cargo xtask device {name}")),
                "{reason}"
            );
        }
        for (check, name) in [
            (DeviceCheck::WindowsA11y, "windows-a11y"),
            (DeviceCheck::WindowsInput { bless: false }, "windows-input"),
            (DeviceCheck::WindowsNotes, "windows-notes"),
            (DeviceCheck::WindowsGallery, "windows-gallery"),
        ] {
            let reason = check.skip_reason().expect("gated");
            assert!(
                reason.ends_with(&format!("; on Windows run: cargo xtask device {name}")),
                "{reason}"
            );
            assert_eq!(check.runs_on_this_host(), cfg!(windows));
        }
        assert_eq!(DeviceCheck::MacosWorkload.skip_reason(), None);
        assert_eq!(
            DeviceCheck::MacosHotReloadLoop { work: "w".into() }.skip_reason(),
            None
        );
    }

    fn windows_a11y_builds_the_probe_and_drives_it_in_process() {
        assert_plan(
            &DeviceCheck::WindowsA11y,
            &[
                "cargo build -p flui --locked --release --example a11y_probe --features material,a11y",
                "uia-client target/release/examples/a11y_probe.exe; if rc=2: echo 'windows-a11y CANNOT VERIFY: input does not reach this desktop (locked or headless), or UI Automation could not be instantiated on this host — details above'; elif rc!=0: echo 'windows-a11y FAILED: the native tree, Invoke, disclosure transition, numeric range or role-family contract failed (tree dumps above)'; exit $rc",
            ],
        );
    }

    /// Each injected session runs in its own capture process and is held
    /// against its golden before the counter is driven; `--bless` replaces
    /// the goldens instead.
    fn windows_input_records_its_sessions_then_drives_the_counter() {
        let session = |child: &str, golden: &str, bless: &str| {
            format!(
                "out=$(FLUI_NATIVE_WINDOW_CONTRACT_CHILD={child} FLUI_NATIVE_RECORDING_DIR=target/native-recordings{bless} cargo test -p flui-platform --locked --test platform_it --features native-capture -- --exact contract::test_window_lifecycle_contract --nocapture 2>&1); printf '%s\\n' \"$out\"; unless exit 0 and 'NATIVE_RECORDING_HELD={golden}' in $out: echo 'windows-input FAILED or CANNOT VERIFY: an injected session did not replay to what the window delivered, no longer matches its golden, or could not run on this desktop (CANNOT_VERIFY, output above)'; exit 1"
            )
        };
        let counter = [
            "cargo build -p flui --locked --release --example a11y_probe --features material,a11y",
            "send-input target/release/examples/a11y_probe.exe; if rc=2: echo 'windows-input CANNOT VERIFY: the probe window could not be kept in the foreground, or UI Automation was unavailable — no input was sent past that point; details above'; elif rc!=0: echo 'windows-input FAILED: a missed click changed the count, or a click on the button or Tab then Enter did not advance it (tree dumps above)'; exit $rc",
        ];
        for (bless, flag) in [(false, ""), (true, " FLUI_NATIVE_RECORDING_BLESS=1")] {
            let touch_and_pen = session(
                "a_captured_native_pointer_session_replays_to_what_it_delivered",
                "win32_injected_touch_and_pen",
                flag,
            );
            let mouse_and_wheel = session(
                "a_captured_native_mouse_session_replays_to_what_it_delivered",
                "win32_sent_mouse_and_wheel",
                flag,
            );
            let recipe: Vec<&str> = [touch_and_pen.as_str(), mouse_and_wheel.as_str()]
                .into_iter()
                .chain(counter)
                .collect();
            assert_plan(&DeviceCheck::WindowsInput { bless }, &recipe);
        }
    }

    /// The ChromeDriver comes first, then the browser test target with it,
    /// the probe built and bound for the page, the BiDi session, and its
    /// recording held against the golden natively; `--bless` writes it.
    fn web_input_runs_the_browser_tests_then_drives_the_probe() {
        let probe: Vec<String> = render(&DeviceCheck::WebInput { bless: false });
        let blessed: Vec<String> = render(&DeviceCheck::WebInput { bless: true });
        assert_eq!(probe.len(), 8, "{probe:#?}");
        assert!(
            probe[0].starts_with("fetch-chromedriver target"),
            "{}",
            probe[0]
        );
        assert!(
            probe[1].contains("CHROMEDRIVER=")
                && probe[1].contains("--target wasm32-unknown-unknown --test web_browser"),
            "{}",
            probe[1]
        );
        assert!(
            probe[2].contains("--example web_input_probe --features native-capture"),
            "{}",
            probe[2]
        );
        assert!(
            probe[4].starts_with("wasm-bindgen --target web"),
            "{}",
            probe[4]
        );
        assert!(probe[5].contains("index.html"), "{}", probe[5]);
        assert!(
            probe[6].starts_with("webdriver-bidi target"),
            "{}",
            probe[6]
        );
        let hold = |plan: &[String]| plan.last().cloned().unwrap_or_default();
        assert!(
            hold(&probe).contains("native_recording::browser_sessions_hold_against_their_goldens")
                && hold(&probe).contains("NATIVE_RECORDING_HELD=web_bidi_mouse_pen_touch")
                && !hold(&probe).contains("FLUI_NATIVE_RECORDING_BLESS"),
            "{}",
            hold(&probe)
        );
        assert!(hold(&blessed).contains("FLUI_NATIVE_RECORDING_BLESS=1"));
    }

    fn windows_notes_builds_the_shared_tree_and_drives_it() {
        assert_plan(
            &DeviceCheck::WindowsNotes,
            &[
                "cargo build -p flui --locked --release --example two_screens --features material,a11y",
                "native-notes target/release/examples/two_screens.exe; if rc=2: echo 'windows-notes CANNOT VERIFY: COM, DPI, foreground or native input was refused (details above)'; elif rc!=0: echo 'windows-notes FAILED: a Notes workflow observation or graceful close failed (tree dump above)'; exit $rc",
            ],
        );
    }

    fn windows_gallery_builds_the_gallery_and_drives_every_screen() {
        assert_plan(
            &DeviceCheck::WindowsGallery,
            &[
                "cargo build -p flui-interaction-gallery --locked --release --features a11y",
                "gallery-session target/release/interaction_gallery.exe target/interaction-gallery; if rc=2: echo 'windows-gallery CANNOT VERIFY: the foreground, the input or UI Automation was refused (details above)'; elif rc!=0: echo 'windows-gallery FAILED: a screen's outcome did not reach the tree or the pixels (summary above)'; exit $rc",
            ],
        );
    }

    /// The page is built with the agent that serves its tree, bound into the
    /// directory the server roots at, and driven from its own index page.
    fn web_gallery_binds_the_page_with_its_agent_and_drives_every_screen() {
        let plan = render(&DeviceCheck::WebGallery);
        let expected: [fn(&str) -> bool; 6] = [
            |step| step.starts_with("fetch-chromedriver target/chromedriver/current/chromedriver"),
            |step| {
                step == "cargo build -p flui-interaction-gallery --locked --release --lib --features agent --target wasm32-unknown-unknown"
            },
            |step| step == "rm -rf target/web-gallery",
            |step| {
                step == "wasm-bindgen --target web --out-dir target/web-gallery/pkg target/wasm32-unknown-unknown/release/flui_interaction_gallery.wasm"
            },
            |step| {
                step == "cp examples/interaction_gallery/index.html target/web-gallery/index.html"
            },
            |step| step.starts_with("gallery-page target/web-gallery target/web-gallery-shots;"),
        ];
        assert_eq!(plan.len(), expected.len(), "{plan:#?}");
        for (index, (step, holds)) in plan.iter().zip(expected).enumerate() {
            assert!(holds(step), "step {index}: {step}");
        }
    }

    /// Runs on any host: the gallery's library is built for the device with
    /// its development agent, then packaged and driven in one native step.
    fn android_gallery_builds_the_library_with_its_agent_and_drives_it() {
        assert!(DeviceCheck::AndroidGallery.runs_on_this_host());
        assert_plan(
            &DeviceCheck::AndroidGallery,
            &[
                "cargo ndk -t arm64-v8a build -p flui-interaction-gallery --locked --lib --features agent",
                "adb-gallery target/aarch64-linux-android/debug/libflui_interaction_gallery.so target/android-gallery target/android-gallery/shots; if rc=2: echo 'android-gallery CANNOT VERIFY: no Android SDK, no attached device, a locked device, or no agent answered (details above)'; elif rc!=0: echo 'android-gallery FAILED: a screen's outcome did not reach the tree or the pixels (summary above)'; exit $rc",
            ],
        );
    }

    fn ios_sim_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::IosSim,
            &[
                "xcrun simctl boot 'iPhone 17 Pro' 2>/dev/null || true",
                "xcrun simctl bootstatus 'iPhone 17 Pro' -b >/dev/null 2>&1 || true",
                "cargo build -p flui --locked --features material --example ios_demo --target aarch64-apple-ios-sim",
                "rm -rf target/ios-sim/IosDemo.app",
                "mkdir -p target/ios-sim/IosDemo.app",
                "cp examples/Info.plist.ios_demo target/ios-sim/IosDemo.app/Info.plist",
                "cp target/aarch64-apple-ios-sim/debug/examples/ios_demo target/ios-sim/IosDemo.app/ios_demo",
                "xcrun simctl install booted target/ios-sim/IosDemo.app",
                "xcrun simctl terminate booted dev.flui.ios-demo 2>/dev/null || true",
                "xcrun simctl launch booted dev.flui.ios-demo >/dev/null",
                "sleep 12",
                "xcrun simctl spawn booted log show --last 5m --process ios_demo > target/ios-sim/app.log 2>/dev/null || true",
                "xcrun simctl io booted screenshot target/ios-sim/screen.png >/dev/null 2>&1 || true",
                "unless target/ios-sim/app.log has a line matching 'Selected GPU:.*Metal' and 'First frame rendered': echo 'IOS_SIM_RESULT=FAIL - arm 1 (static app): expected \"Selected GPU ... Metal\" and \"First frame rendered\"; tail:'; grep 'flui]' target/ios-sim/app.log | tail -20; exit 1",
                "echo 'IOS_SIM arm1=PASS (Metal device created, first frame rendered)'",
                "cargo build -p flui --locked --example animated_box_app --target aarch64-apple-ios-sim",
                "rm -rf target/ios-sim/AnimDemo.app",
                "mkdir -p target/ios-sim/AnimDemo.app",
                "cp examples/Info.plist.ios_anim target/ios-sim/AnimDemo.app/Info.plist",
                "cp target/aarch64-apple-ios-sim/debug/examples/animated_box_app target/ios-sim/AnimDemo.app/animated_box_app",
                "xcrun simctl install booted target/ios-sim/AnimDemo.app",
                "xcrun simctl terminate booted dev.flui.anim-demo 2>/dev/null || true",
                "xcrun simctl launch booted dev.flui.anim-demo >/dev/null",
                "sleep 12",
                "xcrun simctl io booted screenshot target/ios-sim/anim_a.png >/dev/null 2>&1 || true",
                "sleep 2",
                "xcrun simctl io booted screenshot target/ios-sim/anim_b.png >/dev/null 2>&1 || true",
                "cp target/ios-sim/anim_a.png target/ios-sim/anim_a_crop.png",
                "cp target/ios-sim/anim_b.png target/ios-sim/anim_b_crop.png",
                "sips -c 240 240 target/ios-sim/anim_a_crop.png >/dev/null 2>&1 || true",
                "sips -c 240 240 target/ios-sim/anim_b_crop.png >/dev/null 2>&1 || true",
                "ALIVE=$(pgrep -f animated_box_app | wc -l)",
                "xcrun simctl terminate booted dev.flui.anim-demo 2>/dev/null || true",
                "if target/ios-sim/anim_a_crop.png and target/ios-sim/anim_b_crop.png are byte-identical: echo 'IOS_SIM_RESULT=FAIL - arm 2 (animated app): two screenshots 2 s apart are IDENTICAL, so no animation reached the screen (frozen or white) even though frames may be logged; see target/ios-sim/anim_{a,b}.png'; exit 1",
                "if ALIVE < 1: echo 'IOS_SIM_RESULT=FAIL - arm 2 (animated app): the app did not survive to the screenshot pass - request_redraw likely never returned to UIKit and iOS scene-create watchdog killed it'; exit 1",
                "echo 'IOS_SIM arm2=PASS (animated pixels differ, app survived)'",
                "echo 'IOS_SIM_RESULT=PASS (static + animated, screenshots under target/ios-sim/)'",
            ],
        );
    }

    fn every_staged_input_and_driver_is_in_the_tree() {
        let root = repo_root();
        let checks = [
            DeviceCheck::MacosClosePath,
            DeviceCheck::MacosFramePump,
            DeviceCheck::MacosResizeJitter,
            DeviceCheck::MacosIme,
            DeviceCheck::MacosLaunchRender,
            DeviceCheck::MacosLifecycle,
            DeviceCheck::MacosA11y,
            DeviceCheck::MacosWorkload,
            DeviceCheck::MacosHotReloadLoop { work: "w".into() },
            DeviceCheck::IosSim,
            DeviceCheck::IosInputCheck { udid: "U".into() },
            DeviceCheck::IosSafeAreaCheck { udid: "U".into() },
        ];
        let mut inputs = 0;
        for check in &checks {
            for step in check.plan(&context()) {
                let line = step.to_string();
                for word in line
                    .split_whitespace()
                    .map(|word| word.trim_end_matches(';'))
                {
                    let committed =
                        word.starts_with("tools/device-checks/") || word.contains("/Info.plist.");
                    if committed {
                        assert!(root.join(word).is_file(), "{check:?} reads missing {word}");
                        inputs += 1;
                    }
                }
            }
        }
        assert_eq!(inputs, 13, "committed plists and drivers the plans read");
    }

    #[test]
    fn device_check_contract() {
        crate::table_test::run_table(
            "device_check_contract",
            &[
                ("the_cli_is_well_formed", the_cli_is_well_formed as fn()),
                (
                    "every_recipe_name_parses_to_its_check",
                    every_recipe_name_parses_to_its_check as fn(),
                ),
                (
                    "the_input_check_passes_its_driver_arguments_through_verbatim",
                    the_input_check_passes_its_driver_arguments_through_verbatim as fn(),
                ),
                (
                    "off_macos_the_gated_checks_skip_and_the_drivers_decide_the_rest",
                    off_macos_the_gated_checks_skip_and_the_drivers_decide_the_rest as fn(),
                ),
                (
                    "windows_a11y_builds_the_probe_and_drives_it_in_process",
                    windows_a11y_builds_the_probe_and_drives_it_in_process as fn(),
                ),
                (
                    "windows_input_records_its_sessions_then_drives_the_counter",
                    windows_input_records_its_sessions_then_drives_the_counter as fn(),
                ),
                (
                    "web_input_runs_the_browser_tests_then_drives_the_probe",
                    web_input_runs_the_browser_tests_then_drives_the_probe as fn(),
                ),
                (
                    "windows_notes_builds_the_shared_tree_and_drives_it",
                    windows_notes_builds_the_shared_tree_and_drives_it as fn(),
                ),
                (
                    "android_input_drives_the_device_then_holds_what_it_pulled",
                    android_input_drives_the_device_then_holds_what_it_pulled as fn(),
                ),
                (
                    "windows_gallery_builds_the_gallery_and_drives_every_screen",
                    windows_gallery_builds_the_gallery_and_drives_every_screen as fn(),
                ),
                (
                    "web_gallery_binds_the_page_with_its_agent_and_drives_every_screen",
                    web_gallery_binds_the_page_with_its_agent_and_drives_every_screen as fn(),
                ),
                (
                    "android_gallery_builds_the_library_with_its_agent_and_drives_it",
                    android_gallery_builds_the_library_with_its_agent_and_drives_it as fn(),
                ),
                (
                    "ios_sim_matches_its_recipe",
                    ios_sim_matches_its_recipe as fn(),
                ),
                (
                    "every_staged_input_and_driver_is_in_the_tree",
                    every_staged_input_and_driver_is_in_the_tree as fn(),
                ),
            ],
        );
    }
}
