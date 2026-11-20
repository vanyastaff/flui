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
//!   its screen, so the screen is the window, and one of another size is refused. The lane reads
//!   no accessibility tree: UIKit's is served inside the simulator, which no public host API
//!   reads.
//! - **Process.** [`IosSimulatorLane::start`] installs the app when asked and starts it afresh
//!   with a Unix socket in a private directory of the host as its endpoint, which the simulator
//!   shares, and its token, both as `SIMCTL_CHILD_` launch values (ADR-0239); the lane ends the
//!   app with `simctl terminate` when it ends, since an iOS app does not exit when its driver
//!   goes, and removes the socket's directory after it.
//! - **Which simulator.** One named by its UDID ([`device::UDID_ENV`]), listed, available, booted
//!   and running iOS ([`device::choose`]); the lane boots and creates none.
//!
//! Everything here is host code: it builds and its tables run on every host, against a scripted
//! Simulator (`testing::ScriptedXcrun`) as well as the host's `xcrun` ([`xcrun::HostXcrun`]),
//! and starting an app needs a Unix host, whose socket the simulator reaches.

pub mod device;
pub mod screenshot;
pub mod xcode;
pub mod xcrun;

use std::path::PathBuf;
use std::time::Duration;

use flui_protocol::session::{Isolation, Placement};

use self::device::{Simulator, Udid};
use self::xcode::XcodeVersion;
use self::xcrun::{Xcrun, XcrunCommand, XcrunReply, command_failed};
use crate::coordinates::{DevicePoint, WindowGeometry};
use crate::error::{Error, Result};
use crate::lane::{ActivityGuard, Lane, LaneCapabilities, PositionFidelity};
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

/// Where a started app's agent listens, the token it takes and its process.
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct Started {
    /// The Unix socket the app binds, on the host's file system.
    pub endpoint: crate::Endpoint,
    /// The launch token.
    pub token: String,
    /// The app's process id in the simulator, when `simctl launch` printed it.
    pub pid: Option<u32>,
}

/// The iOS Simulator lane for one app on one simulator: see the [module docs](self).
pub struct IosSimulatorLane {
    xcrun: Box<dyn Xcrun>,
    simulator: Simulator,
    xcode: Option<XcodeVersion>,
    bundle: String,
    /// Whether the lane started the app, which it ends at its end.
    app_started: bool,
    /// The private directory the app's socket lives in, removed once the app was ended.
    #[cfg(unix)]
    endpoint: Option<crate::launch::EndpointPlace>,
}

impl IosSimulatorLane {
    /// The lane for the app `bundle` on the simulator `udid`, which `xcrun` reaches: the simulator
    /// listed, available, booted and running iOS, and the Xcode it runs under read.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a bundle identifier that is none; as [`device::choose`]; the
    /// Simulator's failure to list its devices, unavailable when the host has no Xcode.
    pub fn connect(
        mut xcrun: Box<dyn Xcrun>,
        udid: &Udid,
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
        let simulator = device::choose(&simulators, udid)?.clone();
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
            app_started: false,
            #[cfg(unix)]
            endpoint: None,
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

    /// The refusal of every input event: see [`xcode::hid_refusal`].
    fn refusal(&self) -> Error {
        xcode::hid_refusal(self.xcode.as_ref(), &self.simulator.runtime_name())
    }

    /// The screenshot of the simulator's screen, as PNG bytes.
    fn screenshot(&mut self) -> Result<Vec<u8>> {
        self.xcrun.screenshot(&self.simulator.udid, CAPTURE)
    }
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
        LaneCapabilities::new(
            Vec::new(),
            false,
            Isolation::Simulator,
            PositionFidelity::OrderOnly,
        )
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

    fn capture_window(&mut self, placement: &Placement) -> Result<Image> {
        let png = self.screenshot()?;
        screenshot::window_image(&png, placement)
    }
}

impl Drop for IosSimulatorLane {
    fn drop(&mut self) {
        // An iOS app does not exit when its driver's connection closes: the lane ends it, before
        // its socket's directory goes with the lane's fields.
        if self.app_started {
            let terminate = XcrunCommand::Terminate {
                udid: self.simulator.udid.clone(),
                bundle: self.bundle.clone(),
            };
            if let Err(error) = checked(self.xcrun.as_mut(), &terminate, SMALL, SHORT) {
                tracing::error!(
                    lane = LANE,
                    "the app the lane started may still run on simulator {}: {error}",
                    self.simulator.udid
                );
            }
        }
    }
}

impl std::fmt::Debug for IosSimulatorLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosSimulatorLane")
            .field("simulator", &self.simulator)
            .field("xcode", &self.xcode)
            .field("bundle", &self.bundle)
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

/// Start `app` on the simulator `udid` that `xcrun` reaches and drive it: the lane connected, the
/// app started afresh, the driver attached to its socket within `connect_wait`, the lane attached
/// to the driver. The app ends with the session.
///
/// # Errors
///
/// As [`IosSimulatorLane::connect`] and [`IosSimulatorLane::start`]; the app's agent not
/// answering in time.
#[cfg(unix)]
pub fn launch(
    xcrun: Box<dyn Xcrun>,
    udid: &Udid,
    app: &SimulatorApp,
    connect_wait: Duration,
) -> Result<crate::Driver> {
    let mut lane = IosSimulatorLane::connect(xcrun, udid, app.bundle())?;
    let started = lane.start(app)?;
    let driver = crate::Driver::attach(&started.endpoint, &started.token, connect_wait)?;
    driver.with_lane(lane)
}
