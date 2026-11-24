//! The iOS Simulator lane: an app on a booted simulator, started and ended through `xcrun simctl`,
//! and the simulator's own image of its window.
//!
//! - **Input.** None: every touch, key, text, scroll and button is refused as `not_supported`
//!   naming [`xcode::HID_CAPABILITY`], with the Xcode and runtime the simulator runs, before
//!   anything is sent ([`xcode`] says why no channel qualifies). A pause alone is waited out. The
//!   app's own input path (`input.inject`, the synthetic path) reaches it as on any target.
//! - **Isolation.** The simulator itself: nothing the lane does touches the host's desktop, so it
//!   has no guard and takes no permission.
//! - **Reading.** The screen through `simctl io screenshot` ([`screenshot`]): a UIKit window fills
//!   its screen, so the screen is the window, and one of another size is refused. The app's
//!   accessibility tree, the one VoiceOver on iOS reads, through the Mac's accessibility API on
//!   the Simulator's window, where the Simulator bridges the iOS app's elements
//!   ([`accessibility`]); the lane is handed that read ([`IosSimulatorLane::read_through`], the
//!   Mac's on macOS) and matches what it shows to the app's own tree by role, label, state and
//!   bounds ([`tree`], the role table [`roles`]), and presses an element through `AXPress`. A lane
//!   handed none, on any other host, reads no tree.
//! - **Process.** [`IosSimulatorLane::start`] installs the app when asked and starts it afresh
//!   with a Unix socket in a private directory of the host as its endpoint, which the simulator
//!   shares, and its token, both as `SIMCTL_CHILD_` launch values (ADR-0239). When the lane ends
//!   it ends the app with `simctl terminate`, since an iOS app does not exit when its driver goes
//!   (an answer that nothing runs is an app that already exited), and removes the socket's
//!   directory after it.
//! - **Which simulator.** One named by its UDID ([`device::UDID_ENV`]), listed, available, booted
//!   and running iOS ([`device::choose`]); the lane boots and creates none. A session holds its
//!   simulator ([`SimulatorLock`]), since the launch and the terminate name the app by its bundle
//!   identifier and would end another session's copy.
//!
//! Everything here is host code: it builds and its tables run on every host, against a scripted
//! Simulator (`testing::ScriptedXcrun`) as well as the host's `xcrun` ([`xcrun::HostXcrun`]),
//! and starting an app needs a Unix host, whose socket the simulator reaches.

pub mod accessibility;
pub mod device;
pub mod roles;
pub mod screenshot;
pub mod tree;
pub mod xcode;
pub mod xcrun;

use std::path::PathBuf;
use std::time::Duration;

use flui_protocol::session::{Isolation, Placement};
use flui_protocol::tree::Tree;

use self::accessibility::{HostAccessibility, HostScreen, ScreenRequest};
use self::device::{Simulator, Udid};
use self::xcode::XcodeVersion;
use self::xcrun::{Xcrun, XcrunCommand, XcrunReply, command_failed};
use crate::coordinates::{DevicePoint, WindowGeometry};
use crate::error::{Error, Result};
use crate::lane::{AccessibilityApi, ActivityGuard, Lane, LaneCapabilities, PositionFidelity};
use crate::pixels::Image;
use crate::steps::OsEvent;

/// The lane's name, which its refusals and failures carry.
pub const LANE: &str = "ios_simulator";

/// The most bytes a command that prints a few lines may print.
const SMALL: usize = 64 * 1024;

/// How long a short command may take: the version, a terminate.
const SHORT: Duration = Duration::from_secs(30);

/// How long a launch may take: `simctl launch` waits for the app's process to start.
#[cfg(unix)]
const LAUNCH: Duration = Duration::from_secs(90);

/// How long an install may take.
#[cfg(unix)]
const INSTALL: Duration = Duration::from_secs(180);

/// How long a screenshot may take.
const CAPTURE: Duration = Duration::from_secs(30);

/// An app to start on the simulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulatorApp {
    bundle: String,
    app: Option<PathBuf>,
    env: Vec<(String, String)>,
}

impl SimulatorApp {
    /// The app whose bundle identifier is `bundle`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an identifier that is none ([`device::check_bundle_id`]).
    pub fn try_new(bundle: impl Into<String>) -> Result<Self> {
        let bundle = bundle.into();
        device::check_bundle_id(&bundle)?;
        Ok(Self {
            bundle,
            app: None,
            env: Vec::new(),
        })
    }

    /// Install the `.app` bundle at `app`, built for the simulator, before starting the app.
    #[must_use]
    pub fn app_bundle(mut self, app: impl Into<PathBuf>) -> Self {
        self.app = Some(app.into());
        self
    }

    /// Start the app with `key` set to `value` in its environment, as a `SIMCTL_CHILD_` launch
    /// value. The agent's endpoint and token are the lane's own: a value given for either is left
    /// out.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a key that is no environment variable's name: empty, or
    /// holding anything but ASCII letters, digits and `_`, or starting with a digit.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let key = key.into();
        let name = key
            .bytes()
            .next()
            .is_some_and(|first| !first.is_ascii_digit())
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        if !name {
            return Err(Error::invalid_argument(format!(
                "{key:?} is no environment variable's name"
            )));
        }
        self.env.push((key, value.into()));
        Ok(self)
    }

    /// The bundle identifier.
    #[must_use]
    pub fn bundle(&self) -> &str {
        &self.bundle
    }
}

/// Where a started app's agent listens, the token it takes and its process. Its [`Debug`] form
/// leaves the token out.
#[cfg(unix)]
#[derive(Clone)]
pub struct Started {
    /// The Unix socket the app binds, on the host's file system.
    pub endpoint: crate::Endpoint,
    /// The launch token.
    pub token: String,
    /// The app's process id, when `simctl launch` printed it.
    pub pid: Option<u32>,
}

#[cfg(unix)]
impl std::fmt::Debug for Started {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Started")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .field("pid", &self.pid)
            .finish()
    }
}

/// The lock file's name for the simulator `udid`.
fn lock_name(udid: &Udid) -> String {
    format!("flui-driver-simulator-{udid}.lock")
}

/// One session's hold on a simulator: an OS file lock named by its UDID, released when it is
/// dropped or its process ends, however it ends.
///
/// A session starts its app with `simctl launch --terminate-running-process` and ends it with
/// `simctl terminate`, both by bundle identifier, so two sessions on one simulator would end each
/// other's copy: the second is refused instead.
#[derive(Debug)]
#[must_use = "the simulator is held only while the lock is"]
pub struct SimulatorLock {
    udid: Udid,
    file: std::fs::File,
}

impl SimulatorLock {
    /// Hold the simulator `udid`, its lock file in this user's private directory, the one the
    /// OS-input lock lives in.
    ///
    /// # Errors
    ///
    /// As `acquire_in`, the same hold in a directory a test names; [`Error::Io`] when the private directory cannot be made.
    pub fn acquire(udid: &Udid) -> Result<Self> {
        Self::acquire_in(&crate::os_input::private_lock_dir()?, udid)
    }

    /// Hold the simulator `udid`, its lock file in `dir`: [`Self::acquire`]'s directory, or a
    /// test's own (`testing::simulator_lock_in`).
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] while another session holds it; [`Error::Io`] when the lock file cannot be
    /// opened or the OS fails the lock.
    pub(crate) fn acquire_in(dir: &std::path::Path, udid: &Udid) -> Result<Self> {
        let path = dir.join(lock_name(udid));
        let file = crate::os_input::lock_file(&path, || {
            format!(
                "another session drives simulator {udid} ({}); one runs on a simulator at a time",
                path.display()
            )
        })?;
        Ok(Self {
            udid: udid.clone(),
            file,
        })
    }

    /// The simulator held.
    #[must_use]
    pub(crate) fn udid(&self) -> &Udid {
        &self.udid
    }
}

impl Drop for SimulatorLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// What `simctl terminate` prints when no copy of the app runs: the app the lane started has
/// already exited, which is no failure of its end.
pub const NOTHING_TO_TERMINATE: &str = "found nothing to terminate";

/// The iOS Simulator lane for one app on one simulator: see the [module docs](self).
pub struct IosSimulatorLane {
    xcrun: Box<dyn Xcrun>,
    simulator: Simulator,
    xcode: Option<XcodeVersion>,
    bundle: String,
    /// The Mac's accessibility API on the Simulator's window, which the lane reads the app's tree
    /// through; `None` reads no tree.
    accessibility: Option<Box<dyn HostAccessibility>>,
    /// Whether the lane started the app (or may have: a launch that timed out), which it ends at
    /// its end.
    app_started: bool,
    /// The private directory the app's socket lives in, removed once the app was ended.
    #[cfg(unix)]
    endpoint: Option<crate::launch::EndpointPlace>,
    /// The hold on the simulator, released after the app was ended: the last field, so it drops
    /// last.
    _lock: SimulatorLock,
}

impl IosSimulatorLane {
    /// The lane for the app `bundle` on the simulator `lock` holds, which `xcrun` reaches: the
    /// simulator listed, available, booted and running iOS, and the Xcode it runs under read.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a bundle identifier that is none; as [`device::choose`]; the
    /// Simulator's failure to list its devices, unavailable when the host has no Xcode.
    pub fn connect(
        mut xcrun: Box<dyn Xcrun>,
        lock: SimulatorLock,
        bundle: impl Into<String>,
    ) -> Result<Self> {
        let bundle = bundle.into();
        device::check_bundle_id(&bundle)?;
        let listing = checked(
            xcrun.as_mut(),
            &XcrunCommand::ListDevices,
            device::MAX_LISTING,
            SHORT,
        )?;
        let simulators = device::parse_devices(&listing.stdout).map_err(xcrun::lane_error)?;
        let simulator = device::choose(&simulators, lock.udid())?.clone();
        // The version only names the Xcode in a refusal: a host that does not give it still runs
        // the lane, and its refusals say the version is unknown.
        let xcode = checked(xcrun.as_mut(), &XcrunCommand::XcodeVersion, SMALL, SHORT)
            .ok()
            .and_then(|reply| xcode::parse_version(&reply.text()));
        Ok(Self {
            xcrun,
            simulator,
            xcode,
            bundle,
            accessibility: None,
            app_started: false,
            #[cfg(unix)]
            endpoint: None,
            _lock: lock,
        })
    }

    /// The simulator the lane drives.
    #[must_use]
    pub fn simulator(&self) -> &Simulator {
        &self.simulator
    }

    /// The Xcode the simulator runs under, when `xcodebuild -version` gave it.
    #[must_use]
    pub fn xcode(&self) -> Option<&XcodeVersion> {
        self.xcode.as_ref()
    }

    /// Read the app's accessibility tree, and press its elements, through `accessibility`: the
    /// Mac's API on the Simulator's window (`accessibility::MacAccessibility`, on macOS) or a
    /// scripted one. The lane then reports [`AccessibilityApi::SimulatorAx`].
    #[must_use]
    pub fn read_through(mut self, accessibility: Box<dyn HostAccessibility>) -> Self {
        self.accessibility = Some(accessibility);
        self
    }

    /// Which screen a read of the window `placement` describes is of: the simulator's window by
    /// its title, and the app's window it shows.
    fn screen_request(&self, placement: &Placement) -> ScreenRequest {
        ScreenRequest::new(
            accessibility::window_title(&self.simulator),
            self.simulator.udid.clone(),
            placement.size,
            placement.scale,
        )
    }

    /// Refuse a read whose window title names another available simulator as well, asked of
    /// `simctl` before each read, since a simulator made or renamed since the lane connected
    /// counts ([`accessibility::check_title_names_one`]); the UDIDs listed, which a window may name
    /// ([`accessibility::window_identity`]).
    ///
    /// # Errors
    ///
    /// [`Error::Ambiguous`] naming the simulators that share the title; the Simulator's failure to
    /// list its devices.
    fn check_title(&mut self) -> Result<Vec<Udid>> {
        let listing = checked(
            self.xcrun.as_mut(),
            &XcrunCommand::ListDevices,
            device::MAX_LISTING,
            SHORT,
        )?;
        let simulators = device::parse_devices(&listing.stdout).map_err(xcrun::lane_error)?;
        accessibility::check_title_names_one(&simulators, &self.simulator)?;
        Ok(simulators
            .into_iter()
            .map(|simulator| simulator.udid)
            .collect())
    }

    /// The Mac's accessibility API the lane reads through.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `capability` when the lane was handed none.
    fn host_accessibility(&mut self, capability: &str) -> Result<&mut dyn HostAccessibility> {
        match self.accessibility.as_deref_mut() {
            Some(accessibility) => Ok(accessibility),
            None => Err(Error::not_supported(
                capability,
                format!(
                    "the {LANE} lane reads the app through the Mac's accessibility API on the \
                     Simulator's window, which only a run on macOS hands it"
                ),
            )),
        }
    }

    /// The screen of the window `placement` describes as the Simulator serves it, and where its
    /// elements are placed: `capability` names what a lane handed no read refuses.
    fn screen(
        &mut self,
        placement: &Placement,
        capability: &str,
    ) -> Result<(ScreenRequest, HostScreen, tree::Placing)> {
        // Refused before anything is asked when the lane reads through nothing.
        self.host_accessibility(capability)?;
        let listed = self.check_title()?;
        let request = self.screen_request(placement).listed(listed);
        let screen = self.host_accessibility(capability)?.read(&request)?;
        let placing = tree::Placing {
            screen: screen.frame,
            size: placement.size,
            scale: placement.scale,
        };
        Ok((request, screen, placing))
    }

    /// Install `app` when it names a bundle, and start it afresh with a new endpoint and token as
    /// its launch values. The lane ends it, and removes the endpoint's directory, when it ends.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for an app of another bundle than the lane's; the Simulator's
    /// failure to install or launch it.
    #[cfg(unix)]
    pub fn start(&mut self, app: &SimulatorApp) -> Result<Started> {
        use flui_protocol::launch::{ENDPOINT_ENV, TOKEN_ENV};

        if app.bundle != self.bundle {
            return Err(Error::invalid_argument(format!(
                "the lane drives {}, not {}",
                self.bundle, app.bundle
            )));
        }
        let udid = self.simulator.udid.clone();
        if let Some(bundle) = &app.app {
            let install = XcrunCommand::Install {
                udid: udid.clone(),
                app: bundle.clone(),
            };
            checked(self.xcrun.as_mut(), &install, SMALL, INSTALL)?;
        }
        let place = crate::launch::EndpointPlace::new()?;
        let address = place.address.clone();
        // Kept until the app was ended at the lane's end, a failed launch included.
        self.endpoint = Some(place);
        let token = crate::launch::launch_token()?;
        // The agent's values are the lane's whatever the app was given.
        let mut env: Vec<(String, String)> = app
            .env
            .iter()
            .filter(|(key, _)| key != ENDPOINT_ENV && key != TOKEN_ENV)
            .cloned()
            .collect();
        env.push((ENDPOINT_ENV.to_owned(), address.clone()));
        env.push((TOKEN_ENV.to_owned(), token.clone()));
        let launch = XcrunCommand::Launch {
            udid,
            bundle: self.bundle.clone(),
            env,
        };
        // From here on the app may run, a launch that timed out included: the lane ends it at
        // its end whatever comes next.
        self.app_started = true;
        let launched = checked(self.xcrun.as_mut(), &launch, SMALL, LAUNCH)?;
        Ok(Started {
            endpoint: crate::Endpoint::Local(address),
            token,
            pid: device::parse_launched(&launched.text(), &self.bundle),
        })
    }

    /// End the app the lane started: `simctl terminate` by its bundle identifier, which names
    /// only this session's copy while it holds the simulator ([`SimulatorLock`]). An answer of
    /// [`NOTHING_TO_TERMINATE`] is an app that already exited, which is no failure; any other
    /// failure is logged, since no caller is left to tell.
    fn end_app(&mut self) {
        let udid = self.simulator.udid.clone();
        let terminate = XcrunCommand::Terminate {
            udid: udid.clone(),
            bundle: self.bundle.clone(),
        };
        let failure = match self.xcrun.run(&terminate, SMALL, SHORT) {
            Ok(reply) => match reply.failure() {
                None => return,
                Some(_) if exited(&reply) => return,
                Some(failure) => command_failed(&terminate, &reply, &failure),
            },
            Err(error) => error,
        };
        tracing::error!(
            lane = LANE,
            "the app the lane started may still run on simulator {udid}: {failure}"
        );
    }

    /// The refusal of every input event: see [`xcode::hid_refusal`].
    fn refusal(&self) -> Error {
        xcode::hid_refusal(self.xcode.as_ref(), &self.simulator.runtime_name())
    }

    /// The screenshot of the simulator's screen, as PNG bytes.
    fn screenshot(&mut self) -> Result<Vec<u8>> {
        self.xcrun.screenshot(&self.simulator.udid, CAPTURE)
    }
}

/// Whether `simctl terminate` answered that no copy of the app runs.
fn exited(reply: &XcrunReply) -> bool {
    let said = |text: &str| text.to_ascii_lowercase().contains(NOTHING_TO_TERMINATE);
    said(&reply.stderr) || said(&reply.text())
}

/// Run `command` within `limit` bytes and `timeout`, failing on the Simulator's failure.
fn checked(
    xcrun: &mut dyn Xcrun,
    command: &XcrunCommand,
    limit: usize,
    timeout: Duration,
) -> Result<XcrunReply> {
    let reply = xcrun.run(command, limit, timeout)?;
    match reply.failure() {
        Some(failure) => Err(command_failed(command, &reply, &failure)),
        None => Ok(reply),
    }
}

impl Lane for IosSimulatorLane {
    fn name(&self) -> &'static str {
        LANE
    }

    fn capabilities(&self) -> LaneCapabilities {
        let capabilities = LaneCapabilities::new(
            Vec::new(),
            false,
            Isolation::Simulator,
            PositionFidelity::OrderOnly,
        );
        if self.accessibility.is_some() {
            capabilities.accessibility(AccessibilityApi::SimulatorAx)
        } else {
            capabilities
        }
    }

    fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
        None
    }

    fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
        let png = self.screenshot()?;
        let (width, height) = screenshot::png_size(&png)
            .ok_or_else(|| xcrun::lane_error("the Simulator's screenshot is no PNG"))?;
        screenshot::check_fills(placement, width, height)?;
        Ok(crate::lane::screen::window_geometry(
            DevicePoint::new(0, 0),
            placement.size,
            placement.scale,
        ))
    }

    fn check(&mut self, events: &[OsEvent]) -> Result<()> {
        if events
            .iter()
            .any(|event| !matches!(event, OsEvent::Pause(_)))
        {
            return Err(self.refusal());
        }
        Ok(())
    }

    fn send(&mut self, event: &OsEvent) -> Result<()> {
        // `check` refuses it first; a caller that sends directly hears the same.
        let OsEvent::Pause(pause) = event else {
            return Err(self.refusal());
        };
        for slice in crate::lane::pacing::slices(*pause) {
            std::thread::sleep(slice);
        }
        Ok(())
    }

    fn release_all(&mut self) -> Result<()> {
        // The lane sends nothing, so it holds nothing.
        Ok(())
    }

    fn read_tree(&mut self, placement: &Placement, app: &Tree) -> Result<Tree> {
        let (_, screen, placing) = self.screen(placement, "os_accessibility_tree")?;
        let mut projected =
            tree::read(&screen, app, placing).map_err(tree::TreeError::into_error)?;
        for root in &mut projected.roots {
            root.window = Some(placement.window);
        }
        Ok(projected)
    }

    fn invoke(&mut self, placement: &Placement, identifier: &str, app: &Tree) -> Result<()> {
        self.host_accessibility("os_invoke")?;
        if tree::element_of(&app.roots, identifier).is_none() {
            return Err(Error::not_found(format!(
                "the app publishes no element whose identifier is {identifier:?}"
            )));
        }
        let (request, screen, placing) = self.screen(placement, "os_invoke")?;
        let element = tree::matched_element(&screen, app, placing, identifier)
            .map_err(tree::TreeError::into_error)?
            .ok_or_else(|| {
                Error::not_found(format!(
                    "the Simulator shows no element that fits the app's {identifier:?}: \
                     VoiceOver would not reach it"
                ))
            })?
            .clone();
        self.host_accessibility("os_invoke")?
            .press(&request, &element)
    }

    fn capture_window(&mut self, placement: &Placement) -> Result<Image> {
        let png = self.screenshot()?;
        screenshot::window_image(&png, placement)
    }
}

impl Drop for IosSimulatorLane {
    fn drop(&mut self) {
        // An iOS app does not exit when its driver's connection closes: the lane ends it, before
        // its socket's directory and the hold on the simulator go with the lane's fields.
        if std::mem::take(&mut self.app_started) {
            self.end_app();
        }
    }
}

impl std::fmt::Debug for IosSimulatorLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosSimulatorLane")
            .field("simulator", &self.simulator)
            .field("xcode", &self.xcode)
            .field("bundle", &self.bundle)
            .field("reads_tree", &self.accessibility.is_some())
            .field("app_started", &self.app_started)
            .finish_non_exhaustive()
    }
}

/// The host's `xcrun` and the simulator [`device::UDID_ENV`] names: the lane never takes a
/// simulator nobody named, and asks `xcrun` nothing until one is.
///
/// # Errors
///
/// [`Error::Lane`] marked unavailable when no simulator is named; [`Error::InvalidArgument`] for a
/// name that is no UDID.
pub fn host_simulator() -> Result<(xcrun::HostXcrun, Udid)> {
    let named = std::env::var(device::UDID_ENV).ok();
    let Some(named) = named.filter(|named| !named.trim().is_empty()) else {
        return Err(device::unnamed());
    };
    Ok((xcrun::HostXcrun::new(), Udid::try_new(named.trim())?))
}

/// Start `app` on the simulator `udid` that `xcrun` reaches and drive it: the simulator held
/// ([`SimulatorLock::acquire`]), the lane connected, reading through the Mac's accessibility API
/// on macOS (`accessibility::MacAccessibility`), the app started afresh, the driver attached to
/// its socket within `connect_wait`, the lane attached to the driver. The app ends with the
/// session.
///
/// # Errors
///
/// As [`SimulatorLock::acquire`], [`IosSimulatorLane::connect`] and [`IosSimulatorLane::start`];
/// the app's agent not answering in time.
#[cfg(unix)]
pub fn launch(
    xcrun: Box<dyn Xcrun>,
    udid: &Udid,
    app: &SimulatorApp,
    connect_wait: Duration,
) -> Result<crate::Driver> {
    let lock = SimulatorLock::acquire(udid)?;
    let lane = IosSimulatorLane::connect(xcrun, lock, app.bundle())?;
    #[cfg(target_os = "macos")]
    let lane = lane.read_through(Box::new(accessibility::MacAccessibility::new()));
    let mut lane = lane;
    let started = lane.start(app)?;
    let driver = crate::Driver::attach(&started.endpoint, &started.token, connect_wait)?;
    driver.with_lane(lane)
}
