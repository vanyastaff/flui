//! The Android lane: real input into an app on an emulator or a device, driven from the host
//! through `adb`, and the device's own image of the app's window.
//!
//! - **Input.** `input`'s commands ([`held`]): a contact held between events through
//!   `motionevent` on the touch screen or the mouse (the primary button alone: `input` sets no
//!   other), keys as `keyevent` and chords as `keycombination`, text as `input text` in the
//!   printable ASCII its virtual keyboard maps, a scroll as `input mouse scroll` in detents. `input`
//!   drives one pointer and sends no hover, so a second contact is refused before any event, and a
//!   move with nothing down only places the pointer. A pen goes through a `uinput` device the lane
//!   registers ([`uinput`]), so it hovers, presses and leaves as a pen; a device whose shell may not
//!   open `uinput` has no `uinput_device` and its pen is refused naming it. A command the device's
//!   `input` lacks (read from its usage, [`device::parse_input_usage`]) is refused naming it.
//! - **Fidelity.** Order only: Android resamples moves, and each command is its own process, so
//!   the timing between events is not the sequence's.
//! - **Safety.** Before a sequence, `Lane::check` refuses what leaves the app or changes the device
//!   ([`safety`]); before each press, key, text and scroll, one `dumpsys window -a` must show the
//!   window the lane located holding the input focus, by its hash, at the frame and turn the lane
//!   located it at. What the lane holds is released on every exit ([`held`]), and what stays
//!   held is named ([`crate::Error::ReleaseFailed`]).
//! - **Isolation.** An emulator is isolated: nobody holds it. A device is someone's, so it runs
//!   guarded: `getevent` watched for input from any device but the lane's own ([`guard`]).
//! - **Reading.** The window's place and the display's rotation from `dumpsys window`
//!   ([`device::parse_windows`]): the frame of the app's focused window by its hash, on the default
//!   display, which the app's view covers whole. The app's accessibility nodes, the tree TalkBack
//!   reads, from `uiautomator dump` ([`uiautomator`]); the nodes carry no identifier on Android, so
//!   `Lane::read_tree` matches them to the app's own tree by structure, role, name, state and
//!   bounds ([`tree`], [`roles`]), the window located before and after the dump. The dump
//!   suspends every accessibility service while it runs, so the lane takes it only for the tree
//!   and only while none is on, on a device or an emulator ([`safety::check_dump`]); input and
//!   capture never dump. The window's pixels come from `screencap` ([`screencap`]).
//! - **No activation.** `Lane::invoke` is refused: UI Automator's shell performs no action, and
//!   `ACTION_CLICK` reaches a node only from an accessibility service or an instrumentation on the
//!   device, neither of which the lane runs; a tap at the node's center would be the touch path
//!   the input scenarios already take, reported as an activation it is not.
//! - **Process.** [`AndroidLane::start`] installs the app when asked, starts it afresh with its
//!   endpoint and token as intent extras, and forwards a loopback port to its abstract socket; the
//!   lane stops the app and removes the forward when it ends.
//!
//! Everything here is host code: it builds and its tables run on every host, against a scripted
//! device (`testing::ScriptedAdb`) as well as the host's `adb` ([`adb::HostAdb`]).

pub mod adb;
pub mod device;
pub mod getevent;
pub mod guard;
pub mod held;
pub mod keys;
pub mod roles;
pub mod safety;
pub mod screencap;
pub mod tree;
pub mod uiautomator;
pub mod uinput;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::input::PointerDevice;
use flui_protocol::launch::{ENDPOINT_EXTRA, EXIT_ON_DISCONNECT_EXTRA, TOKEN_EXTRA};
use flui_protocol::session::{Isolation, Placement};
use flui_protocol::tree::Tree;

use self::adb::{
    Adb, AdbCommand, AdbReply, AdbStream, Framing, Printed, RunFailure, command_failed, lane_error,
};
use self::device::{DeviceFacts, WindowHash};
use self::guard::GeteventGuard;
use self::held::{Abilities, Command, HeldInput, Part, PenChange};
use self::safety::{Display, Refusal};
use self::uiautomator::{Bounds, UiDump};
use crate::coordinates::{DevicePoint, WindowGeometry};
use crate::error::{Error, Result};
use crate::lane::{
    AccessibilityApi, ActivityGuard, Delivered, Lane, LaneCapabilities, PositionFidelity,
};
use crate::pixels::Image;
use crate::steps::OsEvent;

/// The lane's name, which its refusals and failures carry.
pub const LANE: &str = "android";

/// The most bytes a command that prints a few lines may print.
const SMALL: usize = 64 * 1024;

/// How long a short command may take: a property, a focus line, one `input` command. An `input`
/// command starts a Java process and waits for the app to finish with its event.
const SHORT: Duration = Duration::from_secs(10);

/// How long a dump or a capture may take: `uiautomator` waits for the app to go idle.
const LONG: Duration = Duration::from_secs(30);

/// How long an install may take.
const INSTALL: Duration = Duration::from_secs(180);

/// How many loopback ports the lane tries for its forward when `adb` finds one taken.
pub const FORWARD_ATTEMPTS: usize = 3;

/// How long the pen's device may take to answer a sync.
const SYNC_WITHIN: Duration = Duration::from_secs(5);

/// The API level from which the window manager keeps a focus per display and prints the one
/// holding the device's focus (`mTopFocusedDisplayId=`): Android 10.
const MULTI_DISPLAY_FOCUS_SDK: u32 = 29;

/// How far the app's surface may be from its logical size times its scale, in pixels on each
/// side: a view is laid out in whole pixels.
const SIZE_TOLERANCE: f64 = 2.0;

/// An app to start on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidApp {
    package: String,
    activity: String,
    apk: Option<PathBuf>,
}

impl AndroidApp {
    /// The activity `activity` of the application `package`: `android.app.NativeActivity` for an
    /// app `flui-cli` packages.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a name that is no application id or class.
    pub fn try_new(package: impl Into<String>, activity: impl Into<String>) -> Result<Self> {
        let (package, activity) = (package.into(), activity.into());
        device::check_component(&package, "application id")?;
        device::check_component(&activity, "activity class")?;
        Ok(Self {
            package,
            activity,
            apk: None,
        })
    }

    /// Install `apk` before starting the app.
    #[must_use]
    pub fn apk(mut self, apk: impl Into<PathBuf>) -> Self {
        self.apk = Some(apk.into());
        self
    }

    /// The application id.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }
}

/// Where a started app's agent listens, and the token it takes. Its [`Debug`] form leaves the
/// token out.
#[cfg(any(unix, windows))]
#[derive(Clone)]
pub struct Started {
    /// The loopback port forwarded to the app's abstract socket.
    pub endpoint: crate::Endpoint,
    /// The launch token.
    pub token: String,
}

#[cfg(any(unix, windows))]
impl std::fmt::Debug for Started {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Started")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// The app's window as the lane last found it: which window it is, where, and how the display is
/// turned. Two readings are the same window held still only when all three agree.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Screen {
    /// The window, which must hold the focus for every press, key, text and scroll: a window of
    /// the app's title at the same place is another window.
    window: WindowHash,
    surface: Bounds,
    rotation: u8,
}

/// The pen's `uinput` device, registered.
struct PenDevice {
    stream: Box<dyn AdbStream>,
    next_token: u64,
}

/// What became of one command the lane sent, by whether it reached the device: the one answer
/// the held record and the run's count are both read from.
#[derive(Debug)]
enum CommandOutcome {
    /// The device took it.
    Taken,
    /// It changed nothing on the device: `adb` never started, the device answered by refusing
    /// it, or the pen's device no longer read its report.
    Unsent(Error),
    /// It reached the device and its answer was lost (a deadline, a bound, a pipe, a report not
    /// confirmed): the device may have done it.
    Unanswered(Error),
}

impl CommandOutcome {
    /// Say the command in the run's record: taken, or unanswered and so may have run; one that
    /// never reached the device is not said.
    fn say(&self, delivered: &mut Delivered) {
        match self {
            Self::Taken => delivered.took_command(),
            Self::Unanswered(_) => delivered.unanswered_command(),
            Self::Unsent(_) => {}
        }
    }

    /// The outcome as the lane's answer.
    fn into_result(self) -> Result<()> {
        match self {
            Self::Taken => Ok(()),
            Self::Unsent(error) | Self::Unanswered(error) => Err(error),
        }
    }
}

/// The pen's device ended while a report of it goes out, unless the report is confirmed: on a
/// failure and on an unwind alike, the record of what the pen held goes with the device.
struct PenDeviceGone<'a> {
    held: &'a mut HeldInput,
    confirmed: bool,
}

impl Drop for PenDeviceGone<'_> {
    fn drop(&mut self) {
        if !self.confirmed {
            self.held.pen_device_gone();
        }
    }
}

/// The Android lane for one app on one device: see the [module docs](self).
pub struct AndroidLane {
    adb: Box<dyn Adb>,
    package: String,
    facts: DeviceFacts,
    uinput: bool,
    held: HeldInput,
    guard: Option<GeteventGuard>,
    pen: Option<PenDevice>,
    screen: Option<Screen>,
    /// Whether the lane started the app, which it stops at its end.
    app_started: bool,
    /// The loopback port the lane forwarded to the app, which it removes at its end.
    forwarded: Option<u16>,
    /// Where this session's dumps are written on the device, and how many it took.
    dump_files: uiautomator::DumpFiles,
    dumps: u64,
}

impl AndroidLane {
    /// The lane for the app `package` on the device `adb` drives: what the device is, whether it
    /// takes `uinput` devices, and on a device in someone's hands its guard, started.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a package that is no application id; the device's failure
    /// to answer, unavailable when it is gone.
    pub fn connect(mut adb: Box<dyn Adb>, package: impl Into<String>) -> Result<Self> {
        let package = package.into();
        device::check_component(&package, "application id")?;
        let (sdk, isolation) =
            device::parse_properties(&run_shell(adb.as_mut(), device::PROPERTIES, DATA)?.text());
        safety::admit_isolation(isolation).map_err(Refusal::into_error)?;
        // Unchecked: an older `input` prints its usage with an error and an exit code.
        let usage = adb.run(
            &AdbCommand::Shell(device::INPUT_USAGE.to_owned()),
            SMALL,
            SHORT,
        )?;
        let input = device::parse_input_usage(&usage.text());
        let printed = run_shell(adb.as_mut(), device::DISPLAY, DATA)?.text();
        let display = device::parse_display(&printed).ok_or_else(|| {
            lane_error(format!(
                "`wm size` and `wm density` printed no size the lane reads: {printed:?}"
            ))
        })?;
        let guard = if isolation == Isolation::Device {
            let stream = adb.open(
                &AdbCommand::ShellTerminal(getevent::WATCH.to_owned()),
                Framing::Lines,
            )?;
            Some(GeteventGuard::new(stream))
        } else {
            None
        };
        let uinput = probe_uinput(adb.as_mut());
        let dump_files = uiautomator::DumpFiles::of_session(&crate::launch::launch_token()?[..16]);
        Ok(Self {
            adb,
            package,
            facts: DeviceFacts {
                sdk,
                isolation,
                input,
                display,
            },
            uinput,
            held: HeldInput::new(),
            guard,
            pen: None,
            screen: None,
            app_started: false,
            forwarded: None,
            dump_files,
            dumps: 0,
        })
    }

    /// Install `app` when it names an APK, start it afresh with a new endpoint and token as its
    /// intent extras, and forward a loopback port to the abstract socket it binds. The lane stops
    /// it and removes the forward when it ends.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an app of another package than the lane's; the device's
    /// failure to install, stop, start or forward.
    #[cfg(any(unix, windows))]
    pub fn start(&mut self, app: &AndroidApp) -> Result<Started> {
        if app.package != self.package {
            return Err(Error::invalid_argument(format!(
                "the lane drives {}, not {}",
                self.package, app.package
            )));
        }
        if let Some(apk) = &app.apk {
            let path = apk.to_str().ok_or_else(|| {
                Error::invalid_argument(format!("{} is no UTF-8 path", apk.display()))
            })?;
            let install =
                AdbCommand::Host(["install", "-r", "-t", path].map(str::to_owned).to_vec());
            let run = Run {
                timeout: INSTALL,
                ..STATUS
            };
            checked(self.adb.as_mut(), &install, run)?;
        }
        self.shell(&adb::shell_line(&["am", "force-stop", &self.package]))?;
        let token = crate::launch::launch_token()?;
        let name = format!("flui-agent-{}", &crate::launch::launch_token()?[..16]);
        let endpoint = format!("@{name}");
        let component = format!("{}/{}", app.package, app.activity);
        // From here on the app may run, a failed start included: the lane stops it at its end
        // whatever comes next.
        self.app_started = true;
        self.shell(&adb::shell_line(&[
            "am",
            "start",
            "-W",
            "-n",
            &component,
            "--es",
            ENDPOINT_EXTRA,
            &endpoint,
            "--es",
            TOKEN_EXTRA,
            &token,
            "--es",
            EXIT_ON_DISCONNECT_EXTRA,
            "1",
        ]))?;
        let port = self.forward(&name)?;
        Ok(Started {
            endpoint: crate::Endpoint::Forwarded(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                port,
            ))),
            token,
        })
    }

    /// Forward a loopback port to the abstract socket `name`, and record it for removal at the
    /// lane's end. The lane picks the port (one the OS just handed out and let go of) and asks
    /// with `--no-rebind`, so it never takes over another session's rule: a port someone took in
    /// between is refused by `adb` and another is picked, at most [`FORWARD_ATTEMPTS`] times. A
    /// rule is recorded once `adb` made it, or may have (its answer lost: a timeout, a bound, an
    /// output left open, a pipe), and only then removed; one whose `adb` never started is not.
    #[cfg(any(unix, windows))]
    fn forward(&mut self, name: &str) -> Result<u16> {
        let mut refusals = Vec::new();
        for _ in 0..FORWARD_ATTEMPTS {
            let port = std::net::TcpListener::bind(("127.0.0.1", 0))
                .and_then(|listener| listener.local_addr())
                .map(|address| address.port())?;
            let forward = AdbCommand::Host(vec![
                "forward".to_owned(),
                "--no-rebind".to_owned(),
                format!("tcp:{port}"),
                format!("localabstract:{name}"),
            ]);
            let reply = match self.adb.run(&forward, STATUS.limit, STATUS.timeout) {
                Ok(reply) => reply,
                // A forward whose answer was lost may be in place: it is removed with the lane.
                Err(RunFailure::Unanswered(error)) => {
                    self.forwarded = Some(port);
                    return Err(error);
                }
                Err(RunFailure::NotStarted(error)) => return Err(error),
            };
            let Some(failure) = reply.failure(Printed::Status) else {
                self.forwarded = Some(port);
                return Ok(port);
            };
            let taken = ["cannot bind", "cannot rebind"]
                .iter()
                .any(|said| reply.stderr.contains(said));
            if !taken {
                return Err(command_failed(&forward, &reply, &failure));
            }
            refusals.push(format!("tcp:{port}: {failure}"));
        }
        Err(lane_error(format!(
            "`adb forward` found {FORWARD_ATTEMPTS} loopback ports taken in turn: {}",
            refusals.join("; ")
        )))
    }

    /// Run the shell line `line`, which prints a status, failing on the device's failure.
    fn shell(&mut self, line: &str) -> Result<AdbReply> {
        run_shell(self.adb.as_mut(), line, STATUS)
    }

    /// What the plan reads of the device.
    fn abilities(&self) -> Abilities {
        Abilities {
            input: self.facts.input,
            uinput: self.uinput,
            display: self
                .screen
                .as_ref()
                .map(|screen| self.display(screen.rotation)),
        }
    }

    /// The display as apps see it, turned `rotation` quarter turns.
    fn display(&self, rotation: u8) -> Display {
        let (width, height) = uinput::turned(self.facts.display.natural, rotation);
        Display {
            width,
            height,
            density: self.facts.display.density,
        }
    }

    /// The active window, dumped and read, for its accessibility nodes. The dump suspends every
    /// accessibility service while it runs, so it is taken only once the device or emulator shows
    /// none running.
    fn dump(&mut self) -> Result<UiDump> {
        let settings = Run {
            limit: device::MAX_ACCESSIBILITY_SETTINGS,
            ..DATA
        };
        let printed = run_shell(self.adb.as_mut(), device::ACCESSIBILITY_SETTINGS, settings)?;
        safety::check_dump(&printed.text()).map_err(Refusal::into_error)?;
        self.dumps = self.dumps.saturating_add(1);
        let command = AdbCommand::ExecOut(self.dump_files.dump_line(self.dumps));
        let run = Run {
            printed: Printed::Data,
            limit: uiautomator::MAX_DUMP,
            timeout: LONG,
        };
        let reply = checked(self.adb.as_mut(), &command, run)?.text();
        UiDump::parse(&reply).map_err(|why| match uiautomator::DumpFailure::of(&reply) {
            Some(failure) => Error::busy(failure.to_string()),
            None => lane_error(format!("`uiautomator dump` answered no window: {why}")),
        })
    }

    /// The app's surface `placement` describes, which the lane keeps as where the app's window is,
    /// with that window's hash: the frame of the app's window holding the input focus, from
    /// `dumpsys window`, since the app's view covers its whole window (`FluiView.install`). No
    /// accessibility is read, so no accessibility service is paused: input and capture go on
    /// whatever services run.
    fn find(&mut self, placement: &Placement) -> Result<Screen> {
        let windows = self.read_windows()?;
        let size = (
            placement.size.width * placement.scale,
            placement.size.height * placement.scale,
        );
        let surface = windows.focused_frame().filter(|frame| {
            (f64::from(frame.width()) - size.0).abs() <= SIZE_TOLERANCE
                && (f64::from(frame.height()) - size.1).abs() <= SIZE_TOLERANCE
        });
        let (Some(surface), Some(focus)) = (surface, windows.focus) else {
            return Err(lane_error(format!(
                "{} shows no window of {} x {} pixels for window {}",
                self.package, size.0, size.1, placement.window
            )));
        };
        let screen = Screen {
            window: focus.hash,
            surface,
            rotation: windows.rotation,
        };
        self.screen = Some(screen.clone());
        Ok(screen)
    }

    /// Where the windows are, from one `dumpsys window -a` ([`device::WINDOWS`]), on the default
    /// display, whose focused window must be the app's: the one rule a locate, a press, key, text
    /// and scroll, and a read of the tree are held to.
    ///
    /// Keys and text go to the display holding the device's focus, and `input`, `screencap` and
    /// the rotation are the default display's: another display holding the device's focus, or
    /// the app's focused window on another display or on one named by no number, is refused as
    /// `android_default_display`. From Android 10, and where the API level is unread, the reply
    /// must name the display holding the device's focus; one that names none is refused rather
    /// than taken for the default one.
    fn read_windows(&mut self) -> Result<device::Windows> {
        let run = Run {
            limit: device::MAX_WINDOWS_REPLY,
            ..DATA
        };
        let printed = run_shell(self.adb.as_mut(), device::WINDOWS, run)?.text();
        let windows = device::parse_windows(&printed)
            .map_err(|why| lane_error(format!("`dumpsys window` answered no windows: {why}")))?;
        let elsewhere = |what: String| {
            Error::not_supported(
                "android_default_display",
                format!(
                    "{what}, and the {LANE} lane drives the default display only, which `input`, \
                     `screencap` and the rotation it reads are of"
                ),
            )
        };
        match windows.focused_display {
            Some(display) if display != device::DEFAULT_DISPLAY => {
                return Err(elsewhere(format!(
                    "display {display} holds the device's input focus"
                )));
            }
            None if self.facts.sdk == 0 || self.facts.sdk >= MULTI_DISPLAY_FOCUS_SDK => {
                return Err(lane_error(format!(
                    "`dumpsys window` named no display holding the device's input focus, which \
                     API level {MULTI_DISPLAY_FOCUS_SDK} and later print, on a device of API level \
                     {}",
                    match self.facts.sdk {
                        0 => "unread".to_owned(),
                        sdk => sdk.to_string(),
                    }
                )));
            }
            Some(_) | None => {}
        }
        let focus = windows.focus.as_ref();
        safety::check_focus(focus.map(|name| name.title.as_str()), &self.package)
            .map_err(Refusal::into_error)?;
        if let Some(window) = windows.focused_window()
            && window.display != Some(device::DEFAULT_DISPLAY)
        {
            return Err(elsewhere(format!(
                "the app's window {} holds the focus on display {}",
                window.name.title,
                window
                    .display
                    .map_or_else(|| "none named".to_owned(), |display| display.to_string())
            )));
        }
        Ok(windows)
    }

    /// The window the reading `windows` shows holding the default display's focus, where and how
    /// turned, whatever its size: what a read of the tree compares with the [`Screen`] it located
    /// before its dump.
    fn screen_of(windows: device::Windows) -> Option<Screen> {
        let surface = windows.focused_frame()?;
        Some(Screen {
            window: windows.focus?.hash,
            surface,
            rotation: windows.rotation,
        })
    }

    /// The safety rule before a command that acts on the app, from one reading of the windows:
    /// its window holds the focus, and, once the lane has located the window, the reading shows
    /// the [`Screen`] the lane located. A window of the same title (an activity's window leaving
    /// as it is started again, split screen) is another window, so the app is not in front; the
    /// same window moved, resized or turned would take the command at another place than the
    /// lane computed it for, so it is busy until the lane locates it again. What went out of the
    /// sequence before any of these refusals is said by the run ([`crate::lane::send_guarded`]).
    fn check_focus(&mut self) -> Result<()> {
        let windows = self.read_windows()?;
        let (Some(located), Some(focus)) = (&self.screen, &windows.focus) else {
            return Ok(());
        };
        if focus.hash != located.window {
            return Err(Refusal::NotForeground {
                focus: Some(format!(
                    "{} as window {}, not window {}, the one the lane located",
                    focus.title, focus.hash, located.window
                )),
            }
            .into_error());
        }
        let surface = windows.focused_frame();
        if surface != Some(located.surface) || windows.rotation != located.rotation {
            return Err(Error::busy(format!(
                "the app's window {} moved or the display turned since the {LANE} lane located \
                 it (at {:?} turned {} then, at {surface:?} turned {} now): locate it again",
                located.window, located.surface, located.rotation, windows.rotation
            )));
        }
        Ok(())
    }

    /// Send `command`, recording what the device took. A contact going down is recorded as held
    /// before it goes out, so a send whose answer is lost or that unwinds leaves it for the
    /// release to cancel, and is forgotten only when the command never reached the device or the
    /// device answered by refusing it; any other command is recorded once the device took it, or
    /// as unsure when its answer was lost. What the run counts of it is read from the same
    /// [`CommandOutcome`], so the count and the record agree.
    fn deliver(&mut self, command: &Command) -> CommandOutcome {
        let holds = command.puts_contact_down();
        if holds {
            self.held.unsure(command);
        }
        let line = command.line();
        let through_input = line.is_some();
        let outcome = match line {
            Some(line) => {
                let shell = AdbCommand::Shell(line);
                match self.adb.run(&shell, STATUS.limit, STATUS.timeout) {
                    Ok(reply) => match reply.failure(STATUS.printed) {
                        Some(failure) => {
                            CommandOutcome::Unsent(command_failed(&shell, &reply, &failure))
                        }
                        None => CommandOutcome::Taken,
                    },
                    Err(RunFailure::NotStarted(error)) => CommandOutcome::Unsent(error),
                    Err(RunFailure::Unanswered(error)) => CommandOutcome::Unanswered(error),
                }
            }
            None => self.pen_report(command),
        };
        match &outcome {
            CommandOutcome::Taken if !holds => self.held.took(command),
            // The pen's report that went unconfirmed took its device, and its contacts, with it.
            CommandOutcome::Unanswered(_) if !holds && through_input => self.held.unsure(command),
            CommandOutcome::Unsent(_) if holds => self.held.refused(command),
            CommandOutcome::Taken | CommandOutcome::Unsent(_) | CommandOutcome::Unanswered(_) => {}
        }
        outcome
    }

    /// Send the pen's report `command` through its device, registering it first: unsent when it
    /// was never written to the device, unanswered when the device did not confirm it.
    fn pen_report(&mut self, command: &Command) -> CommandOutcome {
        let events = match self.pen_events(command) {
            Ok(events) => events,
            Err(error) => return CommandOutcome::Unsent(error),
        };
        // The device is out of the lane while its report goes out, and goes back only once the
        // report is confirmed: a report that fails or unwinds ends the device's command, which
        // removes the device and the contacts it held, and the record of them goes with it.
        let mut device = self
            .pen
            .take()
            .expect("BUG: the pen's device was registered by `pen_events`");
        let mut gone = PenDeviceGone {
            held: &mut self.held,
            confirmed: false,
        };
        let id = uinput::STYLUS_ID;
        let outcome = match device.stream.send_line(&uinput::inject(id, &events)) {
            // The command no longer reads its input: the report never reached the device.
            Err(error) => CommandOutcome::Unsent(error),
            // A report written and never confirmed may have been injected: it says so.
            Ok(()) => match device.sync(id) {
                Ok(()) => CommandOutcome::Taken,
                Err(unconfirmed) => CommandOutcome::Unanswered(Error::Timeout {
                    awaited: crate::error::Awaited::Host,
                    effect: Some(Effect::new(EffectKind::MayHaveRun).detail(
                        "the pen's report was written to its `uinput` device, which did not \
                         confirm it",
                    )),
                    message: unconfirmed.to_string(),
                    retry: flui_protocol::error::Retry::Never,
                }),
            },
        };
        let confirmed = matches!(outcome, CommandOutcome::Taken);
        gone.confirmed = confirmed;
        drop(gone);
        if confirmed {
            self.pen = Some(device);
        }
        outcome
    }

    /// The `uinput` events of the pen's report `command`, its device registered first.
    fn pen_events(&mut self, command: &Command) -> Result<Vec<(&'static str, &'static str, i32)>> {
        let Command::Pen { change, at } = command else {
            return Err(lane_error("only the pen's reports go to its device"));
        };
        let rotation = self
            .screen
            .as_ref()
            .ok_or_else(|| lane_error("the lane has not located the app's window"))?
            .rotation;
        let natural = uinput::natural_point(
            *at,
            rotation,
            self.facts.display.natural,
            self.facts.display.physical,
        )
        .ok_or_else(|| Error::outside_target(format!("({}, {}) is off the display", at.x, at.y)))?;
        let in_range = self.held.pen_in_range();
        let events = match change {
            PenChange::Enter | PenChange::Move => uinput::pen_move(natural, in_range),
            PenChange::Press => {
                let mut events = uinput::pen_move(natural, true);
                events.extend(uinput::pen_touch(true));
                events
            }
            PenChange::Lift => uinput::pen_touch(false),
            PenChange::Leave => uinput::pen_leave(),
        };
        if self.pen.is_none() {
            let (width, height) = self.facts.display.physical;
            let axes = uinput::stylus_axes(width, height);
            let registration = uinput::stylus(&axes);
            let mut stream = self.adb.open(
                &AdbCommand::Shell(uinput::COMMAND.to_owned()),
                Framing::JsonObjects,
            )?;
            stream.send_line(&registration.command())?;
            let mut device = PenDevice {
                stream,
                next_token: 1,
            };
            device.sync(registration.id)?;
            self.pen = Some(device);
        }
        Ok(events)
    }

    /// Release what `part` holds: every command of its release tried, and what it still holds
    /// named with its first failure.
    fn release_part(&mut self, part: Part) -> Result<()> {
        let mut first = None;
        for command in self.held.release_plan_of(part) {
            if let Err(error) = self.deliver(&command).into_result() {
                first.get_or_insert(error);
            }
        }
        let held = self.held.held_by(part);
        if held.is_empty() {
            // A failure that left nothing held (the pen's device gone, and its contacts with it)
            // is no failure of the release.
            return Ok(());
        }
        Err(Error::ReleaseFailed {
            lane: LANE.to_owned(),
            held,
            message: first.map_or_else(
                || "the release left it held".to_owned(),
                |error| error.to_string(),
            ),
            first: None,
        })
    }

    /// Wait out `pause` whole, looking at the guard between slices.
    fn pause(&mut self, pause: Duration) -> Result<()> {
        for slice in crate::lane::pacing::slices(pause) {
            if let Some(foreign) = self.guard.as_mut().and_then(ActivityGuard::foreign_input) {
                return Err(Error::InterruptedByUser {
                    lane: LANE.to_owned(),
                    event: foreign.description,
                });
            }
            std::thread::sleep(slice);
        }
        Ok(())
    }
}

impl PenDevice {
    /// Ask device `id` for a sync and wait for its answer: everything sent before it was
    /// injected.
    fn sync(&mut self, id: u32) -> Result<()> {
        let token = self.next_token;
        self.next_token += 1;
        self.stream.send_line(&uinput::sync(id, token))?;
        let deadline = Instant::now() + SYNC_WITHIN;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(lane_error(format!(
                    "the pen's `uinput` device did not answer its sync within {} s",
                    SYNC_WITHIN.as_secs()
                )));
            }
            if let Some(record) = self.stream.next_record(left)?
                && uinput::answers_sync(&record, id, token)
            {
                return Ok(());
            }
        }
    }
}

/// Whether the device lets the shell register a `uinput` device: the probe's device registered,
/// answered its sync and listed by the kernel. The command ends with the probe, which removes
/// the device.
fn probe_uinput(adb: &mut dyn Adb) -> bool {
    let registration = uinput::probe();
    let Ok(stream) = adb.open(
        &AdbCommand::Shell(uinput::COMMAND.to_owned()),
        Framing::JsonObjects,
    ) else {
        return false;
    };
    let mut device = PenDevice {
        stream,
        next_token: 1,
    };
    let registered = device
        .stream
        .send_line(&registration.command())
        .and_then(|()| device.sync(registration.id))
        .is_ok();
    if !registered {
        return false;
    }
    let list = Run {
        limit: uinput::MAX_DEVICE_LIST,
        ..DATA
    };
    run_shell(adb, uinput::DEVICES, list)
        .is_ok_and(|list| uinput::lists_device(&list.text(), uinput::PROBE_NAME))
}

/// How a command is run: what it prints, how much of it is read, how long it may take.
#[derive(Debug, Clone, Copy)]
struct Run {
    printed: Printed,
    limit: usize,
    timeout: Duration,
}

/// A command that prints a status, briefly.
const STATUS: Run = Run {
    printed: Printed::Status,
    limit: SMALL,
    timeout: SHORT,
};

/// A command that prints a few lines of data.
const DATA: Run = Run {
    printed: Printed::Data,
    limit: SMALL,
    timeout: SHORT,
};

/// Run `command` as `run` says, failing on the device's failure.
fn checked(adb: &mut dyn Adb, command: &AdbCommand, run: Run) -> Result<AdbReply> {
    let reply = adb.run(command, run.limit, run.timeout).map_err(|failed| {
        // A command that prints data reads (the windows, the settings, a dump, a capture): its
        // timeout keeps no effect, since it sends nothing that acts on the app, where
        // `adb::timed_out` says the device may have done it, which holds for a command that acts.
        match (run.printed, failed.into_error()) {
            (
                Printed::Data,
                Error::Timeout {
                    awaited,
                    message,
                    retry,
                    ..
                },
            ) => Error::Timeout {
                awaited,
                effect: None,
                message,
                retry,
            },
            (_, other) => other,
        }
    })?;
    match reply.failure(run.printed) {
        Some(failure) => Err(command_failed(command, &reply, &failure)),
        None => Ok(reply),
    }
}

/// Run the shell line `line` as `run` says, failing on the device's failure.
fn run_shell(adb: &mut dyn Adb, line: &str, run: Run) -> Result<AdbReply> {
    checked(adb, &AdbCommand::Shell(line.to_owned()), run)
}

impl Lane for AndroidLane {
    fn name(&self) -> &'static str {
        LANE
    }

    fn capabilities(&self) -> LaneCapabilities {
        let mut pointers = Vec::new();
        if self.facts.input.motion_event {
            pointers.extend([PointerDevice::Touch, PointerDevice::Mouse]);
        }
        if self.uinput {
            pointers.push(PointerDevice::Pen);
        }
        LaneCapabilities::new(
            pointers,
            true,
            self.facts.isolation,
            PositionFidelity::OrderOnly,
        )
        .accessibility(AccessibilityApi::UiAutomator)
    }

    fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
        self.guard
            .as_mut()
            .map(|guard| guard as &mut dyn ActivityGuard)
    }

    fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
        let screen = self.find(placement)?;
        let origin = DevicePoint::new(screen.surface.left, screen.surface.top);
        Ok(crate::lane::screen::window_geometry(
            origin,
            placement.size,
            placement.scale,
        ))
    }

    fn check(&mut self, events: &[OsEvent]) -> Result<()> {
        let abilities = self.abilities();
        let mut held = self.held.clone();
        for event in events {
            for command in held.plan(event, &abilities).map_err(Refusal::into_error)? {
                held.took(&command);
            }
        }
        let down = held.keys_down();
        if !down.is_empty() {
            return Err(Refusal::KeyLeftDown {
                keys: down.into_iter().map(str::to_owned).collect(),
            }
            .into_error());
        }
        Ok(())
    }

    fn send(&mut self, event: &OsEvent) -> Result<()> {
        self.send_counted(event, &mut Delivered::default())
    }

    /// One event is none, one or several commands, each an `input` line or a pen report (a key
    /// down is held for its release, text with a line break is typed around an Enter): each that
    /// reached the device is counted, one whose answer was lost included, as the lane's own record
    /// keeps it.
    fn send_counted(&mut self, event: &OsEvent, delivered: &mut Delivered) -> Result<()> {
        if let OsEvent::Pause(duration) = event {
            return self.pause(*duration);
        }
        let abilities = self.abilities();
        let commands = self
            .held
            .plan(event, &abilities)
            .map_err(Refusal::into_error)?;
        for command in commands {
            if command.needs_focus() {
                self.check_focus()?;
            }
            let outcome = self.deliver(&command);
            outcome.say(delivered);
            outcome.into_result()?;
        }
        Ok(())
    }

    fn release_all(&mut self) -> Result<()> {
        let contact = self.release_part(Part::Contact);
        let pen = self.release_part(Part::Pen);
        // A key a timed-out command may have left down has no release through `input`: it is
        // named once, here, and the next release does not fail on it again.
        let unsure = self.held.take_unsure();
        let keys = if unsure.is_empty() {
            Ok(())
        } else {
            Err(Error::ReleaseFailed {
                lane: LANE.to_owned(),
                held: unsure,
                message: "`input` cannot release a key without pressing it".to_owned(),
                first: None,
            })
        };
        self.held.forget_plans();
        crate::lane::release_outcome([contact, pen, keys])
    }

    fn read_tree(&mut self, placement: &Placement, app: &Tree) -> Result<Tree> {
        // The window is located before the dump, which takes seconds on a phone, and read again
        // after it, whatever its size then: a window replaced, moved or resized or a display
        // turned in between, a quarter turn included, or a dump of another turn, is no snapshot
        // the mapping holds for.
        let before = self.find(placement)?;
        let dump = self.dump()?;
        let after = Self::screen_of(self.read_windows()?);
        if after.as_ref() != Some(&before) || dump.rotation != before.rotation {
            return Err(Error::busy(format!(
                "the app's window changed or the display turned while the {LANE} lane dumped it \
                 (window {} at {:?} turned {} before, {} after, the dump turned {}): read again \
                 once it holds still",
                before.window,
                before.surface,
                before.rotation,
                after.map_or_else(
                    || "no window on the default display".to_owned(),
                    |after| format!(
                        "window {} at {:?} turned {}",
                        after.window, after.surface, after.rotation
                    )
                ),
                dump.rotation
            )));
        }
        let screen = before;
        let display = self.display(screen.rotation);
        let mapping = tree::Mapping {
            origin: DevicePoint::new(screen.surface.left, screen.surface.top),
            display: (display.width, display.height),
        };
        tree::read(&dump, &self.package, app, mapping).map_err(tree::TreeError::into_error)
    }

    fn invoke(&mut self, placement: &Placement, identifier: &str, app: &Tree) -> Result<()> {
        let _ = (placement, app);
        Err(Error::not_supported(
            "os_invoke",
            format!(
                "the {LANE} lane cannot activate {identifier:?} as a screen reader does: \
                 `uiautomator` reads the tree and performs no action, and `ACTION_CLICK` reaches a \
                 node only from an accessibility service or an instrumentation on the device, \
                 neither of which the lane runs; a tap at the node is the touch path, not an \
                 activation"
            ),
        ))
    }

    fn capture_window(&mut self, placement: &Placement) -> Result<Image> {
        let screen = self.find(placement)?;
        let capture = Run {
            printed: Printed::Data,
            limit: screencap::MAX_CAPTURE,
            timeout: LONG,
        };
        // One full-screen buffer at a time: the raw reply is read in place and cut in place; in
        // another pixel format it is dropped before the PNG is asked for, and the PNG's bytes
        // before its decoded image is cut.
        let raw = checked(
            self.adb.as_mut(),
            &AdbCommand::ExecOut(screencap::RAW.to_owned()),
            capture,
        )?;
        let raw = screencap::parse_raw(raw.stdout).map_err(lane_error)?;
        let (width, height, rgba) = if let screencap::Raw::Rgba {
            width,
            height,
            rgba,
        } = raw
        {
            (width, height, rgba)
        } else {
            let png = checked(
                self.adb.as_mut(),
                &AdbCommand::ExecOut(screencap::PNG.to_owned()),
                capture,
            )?;
            screencap::parse_png(&png.stdout).map_err(lane_error)?
        };
        screencap::crop(width, height, rgba, screen.surface).map_err(lane_error)
    }
}

impl Drop for AndroidLane {
    fn drop(&mut self) {
        // The app also exits on its own once its driver's connection closes
        // (`flui.agent.exit_on_disconnect`): a failure here leaves nothing running past that.
        // Each step runs whatever the one before it came to, a panic of the `Adb` (a binding the
        // driver does not own) included; the first panic is resumed once every step ran.
        let mut panicked = flui_foundation::panic::FirstPanic::new();
        let device = self.adb.as_mut();
        if self.app_started {
            panicked.contain(|| {
                run_shell(
                    &mut *device,
                    &adb::shell_line(&["am", "force-stop", &self.package]),
                    STATUS,
                )
            });
        }
        // A dump the host ended at its deadline may have left its files; the session's go with
        // it.
        if self.dumps > 0 {
            panicked.contain(|| run_shell(&mut *device, &self.dump_files.removal(), STATUS));
        }
        if let Some(port) = self.forwarded {
            panicked.contain(|| {
                device.run(
                    &AdbCommand::Host(vec![
                        "forward".to_owned(),
                        "--remove".to_owned(),
                        format!("tcp:{port}"),
                    ]),
                    SMALL,
                    SHORT,
                )
            });
        }
        panicked.resume();
    }
}

impl std::fmt::Debug for AndroidLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidLane")
            .field("package", &self.package)
            .field("facts", &self.facts)
            .field("uinput", &self.uinput)
            .field("held", &self.held)
            .field("guard", &self.guard)
            .finish_non_exhaustive()
    }
}

/// The host's `adb` against the device [`device::SERIAL_ENV`] names: the lane never takes a
/// device nobody named, and asks `adb` nothing until one is.
///
/// # Errors
///
/// [`Error::Lane`] marked unavailable when no device is named, or the named one is not listed and
/// ready.
pub fn host_device() -> Result<adb::HostAdb> {
    let serial = std::env::var(device::SERIAL_ENV).ok();
    let Some(serial) = serial.filter(|serial| !serial.trim().is_empty()) else {
        return Err(device::unnamed());
    };
    let mut host = adb::HostAdb::locate(serial.clone())?;
    let listing = checked(&mut host, &AdbCommand::Devices, DATA)?;
    device::choose(&device::parse_devices(&listing.text()), Some(&serial))?;
    Ok(host)
}

/// Start `app` on the device `adb` drives and drive it: the lane connected, the app started
/// afresh, the driver attached to its forwarded endpoint within `connect_wait`, the lane
/// attached to the driver. The app ends with the session.
///
/// # Errors
///
/// As [`AndroidLane::connect`] and [`AndroidLane::start`]; the app's agent not answering in time.
#[cfg(any(unix, windows))]
pub fn launch(
    adb: Box<dyn Adb>,
    app: &AndroidApp,
    connect_wait: Duration,
) -> Result<crate::Driver> {
    let mut lane = AndroidLane::connect(adb, app.package())?;
    let started = lane.start(app)?;
    let driver = crate::Driver::attach(&started.endpoint, &started.token, connect_wait)?;
    driver.with_lane(lane)
}
