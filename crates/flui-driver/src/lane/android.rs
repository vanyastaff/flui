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
//!   ([`safety`]); before each press, key, text and scroll, the app's window must hold the input
//!   focus. What the lane holds is released on every exit ([`held`]), and what stays held is named
//!   ([`crate::Error::ReleaseFailed`]).
//! - **Isolation.** An emulator is isolated: nobody holds it. A device is someone's, so it runs
//!   guarded: `getevent` watched for input from any device but the lane's own ([`guard`]).
//! - **Reading.** The window's place, the display's rotation and the app's accessibility nodes,
//!   the tree TalkBack reads, from `uiautomator dump` ([`uiautomator`]); the nodes carry no
//!   identifier on Android, so `Lane::read_tree` matches them to the app's own tree by structure,
//!   role, name, state and bounds ([`tree`], [`roles`]). The dump suspends every accessibility
//!   service while it runs, so the lane dumps nothing while one is on ([`safety::check_dump`]). The window's pixels come from `screencap` ([`screencap`]).
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

use flui_protocol::input::PointerDevice;
use flui_protocol::launch::{ENDPOINT_EXTRA, EXIT_ON_DISCONNECT_EXTRA, TOKEN_EXTRA};
use flui_protocol::session::{Isolation, Placement};
use flui_protocol::tree::Tree;

use self::adb::{
    Adb, AdbCommand, AdbReply, AdbStream, Framing, Printed, command_failed, lane_error,
};
use self::device::DeviceFacts;
use self::guard::GeteventGuard;
use self::held::{Abilities, Command, HeldInput, Part, PenChange};
use self::safety::{Display, Refusal};
use self::uiautomator::{Bounds, UiDump};
use crate::coordinates::{DevicePoint, WindowGeometry};
use crate::error::{Error, Result};
use crate::lane::{AccessibilityApi, ActivityGuard, Lane, LaneCapabilities, PositionFidelity};
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

/// The app's window as the lane last found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Screen {
    surface: Bounds,
    rotation: u8,
}

/// The pen's `uinput` device, registered.
struct PenDevice {
    stream: Box<dyn AdbStream>,
    next_token: u64,
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
    /// Where this session's window dumps are written on the device.
    dump_path: String,
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
        let dump_path = uiautomator::dump_path(&crate::launch::launch_token()?[..16]);
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
            dump_path,
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
    /// rule is recorded once `adb` made it, or may have (a timeout), and only then removed.
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
                Err(error @ Error::Timeout { .. }) => {
                    self.forwarded = Some(port);
                    return Err(error);
                }
                Err(error) => return Err(error),
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
            display: self.screen.map(|screen| self.display(screen.rotation)),
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

    /// The active window, dumped and read, once the device shows no accessibility service
    /// running: the dump suspends every service while it runs.
    fn dump(&mut self) -> Result<UiDump> {
        let settings = Run {
            limit: device::MAX_ACCESSIBILITY_SETTINGS,
            ..DATA
        };
        let printed = run_shell(self.adb.as_mut(), device::ACCESSIBILITY_SETTINGS, settings)?;
        safety::check_dump(&printed.text()).map_err(Refusal::into_error)?;
        let command = AdbCommand::ExecOut(uiautomator::dump_line(&self.dump_path));
        let run = Run {
            printed: Printed::Data,
            limit: uiautomator::MAX_DUMP,
            timeout: LONG,
        };
        let reply = checked(self.adb.as_mut(), &command, run)?;
        UiDump::parse(&reply.text())
            .map_err(|why| lane_error(format!("`uiautomator dump` answered no window: {why}")))
    }

    /// The app's surface `placement` describes, found in the active window.
    fn find(&mut self, placement: &Placement) -> Result<Screen> {
        let dump = self.dump()?;
        self.surface_in(&dump, placement)
    }

    /// The app's surface `placement` describes, found in `dump`, which the lane keeps as where the
    /// app's window is.
    fn surface_in(&mut self, dump: &UiDump, placement: &Placement) -> Result<Screen> {
        let size = (
            placement.size.width * placement.scale,
            placement.size.height * placement.scale,
        );
        let Some(surface) = dump.surface_of(&self.package, size, SIZE_TOLERANCE) else {
            let packages = dump.packages();
            if !packages.contains(&self.package.as_str()) {
                return Err(Refusal::NotForeground {
                    focus: Some(packages.join(", ")),
                }
                .into_error());
            }
            return Err(lane_error(format!(
                "{} shows no view of {} x {} pixels for window {}",
                self.package, size.0, size.1, placement.window
            )));
        };
        let screen = Screen {
            surface,
            rotation: dump.rotation,
        };
        self.screen = Some(screen);
        Ok(screen)
    }

    /// The safety rule before a command that acts on the app: its window holds the focus.
    fn check_focus(&mut self) -> Result<()> {
        let printed = run_shell(self.adb.as_mut(), device::FOCUS, DATA)?.text();
        let focus = device::parse_focus(&printed);
        safety::check_focus(focus.as_deref(), &self.package).map_err(Refusal::into_error)
    }

    /// Send `command`, recording what the device took.
    fn deliver(&mut self, command: &Command) -> Result<()> {
        let sent = match command.line() {
            Some(line) => run_shell(self.adb.as_mut(), &line, STATUS).map(drop),
            None => self.pen_report(command),
        };
        match &sent {
            Ok(()) => self.held.took(command),
            Err(Error::Timeout { .. }) => self.held.unsure(command),
            Err(_) => {}
        }
        sent
    }

    /// Send the pen's report `command` through its device, registering it first.
    fn pen_report(&mut self, command: &Command) -> Result<()> {
        let Command::Pen { change, at } = command else {
            return Err(lane_error("only the pen's reports go to its device"));
        };
        let screen = self
            .screen
            .ok_or_else(|| lane_error("the lane has not located the app's window"))?;
        let natural = uinput::natural_point(
            *at,
            screen.rotation,
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
        let device = self
            .pen
            .as_mut()
            .expect("BUG: the pen's device was registered just above");
        let id = uinput::STYLUS_ID;
        let sent = device
            .stream
            .send_line(&uinput::inject(id, &events))
            .and_then(|()| device.sync(id));
        if sent.is_err() {
            // The device's command is gone or stuck: ending it removes the device, and the kernel
            // the contacts it held.
            self.pen = None;
            self.held.pen_device_gone();
        }
        sent
    }

    /// Release what `part` holds: every command of its release tried, and what it still holds
    /// named with its first failure.
    fn release_part(&mut self, part: Part) -> Result<()> {
        let mut first = None;
        for command in self.held.release_plan_of(part) {
            if let Err(error) = self.deliver(&command) {
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
    let reply = adb.run(command, run.limit, run.timeout)?;
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
            self.deliver(&command)?;
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
        let dump = self.dump()?;
        let screen = self.surface_in(&dump, placement)?;
        let display = self.display(screen.rotation);
        let mapping = tree::Mapping {
            origin: DevicePoint::new(screen.surface.left, screen.surface.top),
            display: (display.width, display.height),
        };
        tree::read(&dump, &self.package, app, mapping).map_err(tree::TreeError::into_error)
    }

    fn invoke(&mut self, placement: &Placement, identifier: &str) -> Result<()> {
        let _ = placement;
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
        if self.app_started {
            let _ = run_shell(
                self.adb.as_mut(),
                &adb::shell_line(&["am", "force-stop", &self.package]),
                STATUS,
            );
        }
        if let Some(port) = self.forwarded {
            let _ = self.adb.run(
                &AdbCommand::Host(vec![
                    "forward".to_owned(),
                    "--remove".to_owned(),
                    format!("tcp:{port}"),
                ]),
                SMALL,
                SHORT,
            );
        }
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
