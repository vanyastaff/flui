//! End-to-end checks on Apple hardware: build a probe, stage it the way the
//! platform needs it, and run it or its driver from `tools/device-checks/`.
//!
//! The drivers stay Python and Swift: they only run on a Mac, so only a Mac
//! can port them with evidence. This module owns what the recipes around them
//! did — the build, the staging and the verdict on a probe's output — as a
//! plan computed first and executed second, so a test on any host can hold
//! each check against the commands it replaced.
//!
//! On another host a check that needs macOS says why it is skipped and exits
//! 0. `macos-workload` and `macos-hot-reload-loop` leave that decision to
//! their drivers (the first skips with 0, the second refuses with 1). Paths
//! are relative to the repository root, where every step runs.

mod plan;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use plan::{Announce, Program, Run, Sink, Step};

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

/// `cargo xtask device <check>`: run a macOS or iOS device check.
pub(crate) fn device(args: &DeviceArgs) -> anyhow::Result<ExitCode> {
    if !cfg!(target_os = "macos")
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
}

impl DeviceCheck {
    /// Why this check does nothing off macOS, or `None` when its driver
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
            Self::MacosWorkload | Self::MacosHotReloadLoop { .. } => return None,
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
        }
    }
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

    #[test]
    fn the_cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
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

    #[test]
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

    #[test]
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
        assert_eq!(DeviceCheck::MacosWorkload.skip_reason(), None);
        assert_eq!(
            DeviceCheck::MacosHotReloadLoop { work: "w".into() }.skip_reason(),
            None
        );
    }

    #[test]
    fn macos_close_path_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosClosePath,
            &[
                "cargo build -p flui-platform --locked --example close_path_probe",
                "rm -rf target/macos-close-path/ClosePathProbe.app",
                "mkdir -p target/macos-close-path/ClosePathProbe.app/Contents/MacOS",
                "cp crates/flui-platform/examples/Info.plist.close_path_probe target/macos-close-path/ClosePathProbe.app/Contents/Info.plist",
                "cp target/debug/examples/close_path_probe target/macos-close-path/ClosePathProbe.app/Contents/MacOS/close_path_probe",
                "out=$(RUST_LOG=info target/macos-close-path/ClosePathProbe.app/Contents/MacOS/close_path_probe 2>&1); printf '%s\\n' \"$out\"; unless exit 0 and 'CLOSE_PATH_PROBE_RESULT=PASS' in $out: echo 'macos-close-path FAILED: probe exit code or PASS marker missing (output above)'; exit 1",
            ],
        );
    }

    #[test]
    fn macos_frame_pump_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosFramePump,
            &[
                "cargo build -p flui-platform --locked --example frame_pump_probe",
                "rm -rf target/macos-frame-pump/FramePumpProbe.app",
                "mkdir -p target/macos-frame-pump/FramePumpProbe.app/Contents/MacOS",
                "cp crates/flui-platform/examples/Info.plist.frame_pump_probe target/macos-frame-pump/FramePumpProbe.app/Contents/Info.plist",
                "cp target/debug/examples/frame_pump_probe target/macos-frame-pump/FramePumpProbe.app/Contents/MacOS/frame_pump_probe",
                "out=$(RUST_LOG=info target/macos-frame-pump/FramePumpProbe.app/Contents/MacOS/frame_pump_probe 2>&1); printf '%s\\n' \"$out\"; unless exit 0 and 'FRAME_PUMP_PROBE_RESULT=PASS' in $out: echo 'macos-frame-pump FAILED: probe exit code or PASS marker missing (output above)'; exit 1",
            ],
        );
    }

    #[test]
    fn macos_resize_jitter_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosResizeJitter,
            &[
                "cargo build -p flui --locked --example resize_jitter_probe",
                "rm -rf target/macos-resize-jitter/ResizeJitterProbe.app",
                "mkdir -p target/macos-resize-jitter/ResizeJitterProbe.app/Contents/MacOS",
                "cp examples/Info.plist.resize_jitter_probe target/macos-resize-jitter/ResizeJitterProbe.app/Contents/Info.plist",
                "cp target/debug/examples/resize_jitter_probe target/macos-resize-jitter/ResizeJitterProbe.app/Contents/MacOS/resize_jitter_probe",
                "out=$(RUST_LOG=info target/macos-resize-jitter/ResizeJitterProbe.app/Contents/MacOS/resize_jitter_probe 2>&1); printf '%s\\n' \"$out\"; unless exit 0 and 'RESIZE_JITTER_PROBE_RESULT=PASS' and 'RESIZE_JITTER_PROBE_STALE=0' in $out: echo 'macos-resize-jitter FAILED: probe exit code, PASS marker, or the zero stale-size marker is missing (output above)'; exit 1",
            ],
        );
    }

    #[test]
    fn macos_ime_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosIme,
            &[
                "cargo build -p flui-platform --locked --example ime_probe",
                "rm -rf target/macos-ime/ImeProbe.app",
                "mkdir -p target/macos-ime/ImeProbe.app/Contents/MacOS",
                "cp crates/flui-platform/examples/Info.plist.ime_probe target/macos-ime/ImeProbe.app/Contents/Info.plist",
                "cp target/debug/examples/ime_probe target/macos-ime/ImeProbe.app/Contents/MacOS/ime_probe",
                "out=$(RUST_LOG=info target/macos-ime/ImeProbe.app/Contents/MacOS/ime_probe 2>&1); printf '%s\\n' \"$out\"; unless exit 0 and 'IME_PROBE_RESULT=PASS' in $out: echo 'macos-ime FAILED: probe exit code or PASS marker missing (output above)'; exit 1",
            ],
        );
    }

    #[test]
    fn macos_launch_render_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosLaunchRender,
            &[
                "cargo build -p flui --locked --example colored_box_app",
                "python tools/device-checks/check-macos-launch-render.py target/debug/examples/colored_box_app --runs 5 --expect 240,0,0; if rc=2: echo 'macos-launch-render CANNOT VERIFY: this host could not take the measurement (Screen Recording not granted, or swiftc missing) — a denied capture is NOT a blank window, so nothing was decided; details above'; elif rc!=0: echo 'macos-launch-render FAILED: a launch route was refused, put no window on screen, or put up a window that stayed blank for the whole settle - details and images above'; exit $rc",
            ],
        );
    }

    #[test]
    fn macos_lifecycle_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosLifecycle,
            &[
                "cargo build -p flui --locked --release --example lifecycle_probe --features material",
                "out=$(RUST_LOG=warn target/release/examples/lifecycle_probe 2>&1); printf '%s\\n' \"$out\"; unless exit 0 and 'LIFECYCLE_PROBE_RESULT=PASS' in $out: echo 'macos-lifecycle FAILED: a phase was over budget or the probe did not finish (phase lines above)'; exit 1",
            ],
        );
    }

    #[test]
    fn macos_a11y_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosA11y,
            &[
                "python -B tools/device-checks/check-macos-a11y.py; if rc=2: echo 'macos-a11y CANNOT VERIFY: this host could not take the measurement (accessibility trust not granted, or swiftc missing) — details above'; elif rc!=0: echo 'macos-a11y FAILED: the button was not in the accessibility tree, AXPress was refused, or the count did not advance (tree dumps above)'; exit $rc",
            ],
        );
    }

    #[test]
    fn macos_workload_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::MacosWorkload,
            &["python -B tools/device-checks/check-macos-workload.py; exit $rc"],
        );
    }

    #[test]
    fn macos_hot_reload_loop_matches_its_recipe_and_names_the_built_cli() {
        assert_plan(
            &DeviceCheck::MacosHotReloadLoop {
                work: "target/hot-reload-loop/work".into(),
            },
            &[
                "cargo build -p flui-cli --locked",
                "python -B tools/device-checks/check-hot-reload-loop.py target/hot-reload-loop/work --cli target/debug/flui; exit $rc",
            ],
        );
    }

    #[test]
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

    #[test]
    fn ios_sim_boots_the_simulator_the_environment_names() {
        let context = Context {
            target_dir: "target".into(),
            sim_device: "iPad Pro 13-inch (M4)".into(),
        };
        let plan = DeviceCheck::IosSim.plan(&context);
        assert_eq!(
            plan[0].to_string(),
            "xcrun simctl boot 'iPad Pro 13-inch (M4)' 2>/dev/null || true"
        );
    }

    #[test]
    fn ios_input_check_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::IosInputCheck {
                udid: "UDID-1".into(),
            },
            &[
                "cargo build -p flui --locked --features material --example ios_demo --target aarch64-apple-ios-sim",
                "rm -rf target/ios-input/IosDemo.app",
                "mkdir -p target/ios-input/IosDemo.app",
                "cp examples/Info.plist.ios_demo target/ios-input/IosDemo.app/Info.plist",
                "cp target/aarch64-apple-ios-sim/debug/examples/ios_demo target/ios-input/IosDemo.app/ios_demo",
                "xcrun simctl uninstall UDID-1 dev.flui.ios-demo 2>/dev/null || true",
                "python -B tools/device-checks/check-ios-input.py UDID-1 target/ios-input/IosDemo.app; if rc=2: echo 'ios-input-check CANNOT VERIFY: this host could not take the measurement (no Xcode toolchain, the simulator was not booted, or the probe produced no report) - nothing was decided about the framework; details above'; elif rc!=0: echo 'ios-input-check FAILED: a real touch did not reach a widget, or the state it changed did not survive Home/return, or a control did not behave - details and per-stage screenshots above'; exit $rc",
            ],
        );
    }

    #[test]
    fn ios_safe_area_check_matches_its_recipe() {
        assert_plan(
            &DeviceCheck::IosSafeAreaCheck {
                udid: "UDID-1".into(),
            },
            &[
                "python -B tools/device-checks/check-ios-safe-area.py UDID-1 target/ios-safe-area-check; exit $rc",
            ],
        );
    }

    #[test]
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
}
