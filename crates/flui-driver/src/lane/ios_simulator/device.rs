//! Which simulator the lane drives and what `simctl` says of it, read as untrusted input.
//!
//! - A simulator is named by its [`Udid`], checked before it becomes an argument; the lane takes
//!   no simulator nobody named ([`UDID_ENV`]).
//! - `simctl list devices --json` is read within [`MAX_LISTING`] bytes ([`parse_devices`]); an
//!   entry the lane cannot read is left out on its own, never the whole listing with it.
//! - `simctl launch` prints `<bundle>: <pid>` for the process it started ([`parse_launched`]).

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use super::xcrun::unavailable;
use crate::error::{Error, Result};

/// The environment variable that names the simulator the lane drives, by its UDID.
pub const UDID_ENV: &str = "FLUI_IOS_SIMULATOR_UDID";

/// The most bytes `simctl list devices --json` may print: thousands of simulators.
pub const MAX_LISTING: usize = 4 << 20;

/// The most bytes a bundle identifier may have.
const MAX_BUNDLE_ID: usize = 255;

/// The prefix of an iOS runtime's identifier: `com.apple.CoreSimulator.SimRuntime.iOS-18-0`.
const IOS_RUNTIME: &str = "com.apple.CoreSimulator.SimRuntime.iOS-";

/// A simulator's UDID: a UUID in its 8-4-4-4-12 hexadecimal form, which `simctl` prints in upper
/// case. Two UDIDs are the same simulator when they are equal ignoring case, so one is held in
/// upper case.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Udid(String);

impl Udid {
    /// `value` as a UDID.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for anything but a UUID's 8-4-4-4-12 hexadecimal form.
    pub fn try_new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let groups: Vec<&str> = value.split('-').collect();
        let lengths: Vec<usize> = groups.iter().map(|group| group.len()).collect();
        let hex = groups
            .iter()
            .all(|group| group.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if lengths != [8, 4, 4, 4, 12] || !hex {
            return Err(Error::invalid_argument(format!(
                "{value:?} is no simulator UDID: a UDID is a UUID such as \
                 1F2E3D4C-5B6A-4798-8A7B-6C5D4E3F2A1B"
            )));
        }
        Ok(Self(value.to_ascii_uppercase()))
    }

    /// The UDID, in upper case.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Udid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Refuse a bundle identifier that is none: empty, past 255 bytes, or holding anything but ASCII
/// letters, digits, `-` and `.` (`CFBundleIdentifier`'s characters), or starting with `-` or `.`,
/// which a tool would read as an option or a hidden name.
///
/// # Errors
///
/// [`Error::InvalidArgument`] naming the identifier.
pub fn check_bundle_id(bundle: &str) -> Result<()> {
    let characters = bundle
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.');
    let first = bundle.bytes().next();
    let valid = characters
        && bundle.len() <= MAX_BUNDLE_ID
        && first.is_some_and(|first| first != b'-' && first != b'.');
    if valid {
        Ok(())
    } else {
        Err(Error::invalid_argument(format!(
            "{bundle:?} is no bundle identifier: ASCII letters, digits, `-` and `.`, as \
             `dev.flui.scenario`"
        )))
    }
}

/// What state a simulator is in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceState {
    /// Booted: its apps run.
    Booted,
    /// Shut down.
    Shutdown,
    /// Any other state `simctl` names (`Booting`, `Shutting Down`, `Creating`).
    Other(String),
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Booted => f.write_str("Booted"),
            Self::Shutdown => f.write_str("Shutdown"),
            Self::Other(state) => f.write_str(state),
        }
    }
}

/// A simulator as `simctl list devices` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Simulator {
    /// Its UDID.
    pub udid: Udid,
    /// Its name: `iPhone 16`.
    pub name: String,
    /// The identifier of the runtime it runs: `com.apple.CoreSimulator.SimRuntime.iOS-18-0`.
    pub runtime: String,
    /// Its state.
    pub state: DeviceState,
    /// Whether its runtime and device type are installed, so it can boot.
    pub available: bool,
}

impl Simulator {
    /// The runtime it runs, for people: `iOS 18.0`, or its identifier when it is no iOS runtime.
    #[must_use]
    pub fn runtime_name(&self) -> String {
        runtime_name(&self.runtime).unwrap_or_else(|| self.runtime.clone())
    }
}

#[derive(Deserialize)]
struct Listing {
    devices: BTreeMap<String, Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct Entry {
    udid: String,
    name: String,
    state: String,
    #[serde(rename = "isAvailable", default)]
    is_available: bool,
}

/// The simulators `printed`, the JSON of `simctl list devices --json`, lists. An entry the lane
/// cannot read (a field missing, a UDID that is none) is left out on its own.
///
/// # Errors
///
/// Why `printed` is no listing: not JSON, or no `devices` object of runtimes.
pub fn parse_devices(printed: &[u8]) -> std::result::Result<Vec<Simulator>, String> {
    let listing: Listing = serde_json::from_slice(printed)
        .map_err(|error| format!("`simctl list devices --json` printed no listing: {error}"))?;
    let mut simulators = Vec::new();
    for (runtime, entries) in listing.devices {
        for entry in entries {
            let Ok(entry) = serde_json::from_value::<Entry>(entry) else {
                continue;
            };
            let Ok(udid) = Udid::try_new(entry.udid) else {
                continue;
            };
            let state = match entry.state.as_str() {
                "Booted" => DeviceState::Booted,
                "Shutdown" => DeviceState::Shutdown,
                _ => DeviceState::Other(entry.state),
            };
            simulators.push(Simulator {
                udid,
                name: entry.name,
                runtime: runtime.clone(),
                state,
                available: entry.is_available,
            });
        }
    }
    Ok(simulators)
}

/// The simulator `udid` among `simulators`, when the lane can drive it: listed, available, booted
/// and running iOS.
///
/// # Errors
///
/// [`Error::Lane`] marked unavailable when it is not listed, not available or not booted, which
/// the host's own setup decides; [`Error::NotSupported`] naming `ios_simulator_runtime` for a
/// simulator of another OS (tvOS, watchOS, visionOS), which FLUI does not run on.
pub fn choose<'a>(simulators: &'a [Simulator], udid: &Udid) -> Result<&'a Simulator> {
    let simulator = simulators
        .iter()
        .find(|simulator| simulator.udid == *udid)
        .ok_or_else(|| {
            unavailable(format!(
                "no simulator {udid} is listed by `xcrun simctl list devices`"
            ))
        })?;
    if runtime_name(&simulator.runtime).is_none() {
        return Err(Error::not_supported(
            "ios_simulator_runtime",
            format!(
                "simulator {udid} ({}) runs {}, which is no iOS runtime",
                simulator.name, simulator.runtime
            ),
        ));
    }
    if !simulator.available {
        return Err(unavailable(format!(
            "simulator {udid} ({}, {}) is not available: its runtime or device type is not \
             installed",
            simulator.name,
            simulator.runtime_name()
        )));
    }
    if simulator.state != DeviceState::Booted {
        return Err(unavailable(format!(
            "simulator {udid} ({}, {}) is {}, not booted: boot it with `xcrun simctl boot {udid}`",
            simulator.name,
            simulator.runtime_name(),
            simulator.state
        )));
    }
    Ok(simulator)
}

/// The iOS runtime `identifier` names, for people (`iOS 18.0` for
/// `com.apple.CoreSimulator.SimRuntime.iOS-18-0`); `None` for an identifier of another OS or a
/// version that is not numbers.
#[must_use]
pub fn runtime_name(identifier: &str) -> Option<String> {
    let version = identifier.strip_prefix(IOS_RUNTIME)?;
    let parts: Vec<&str> = version.split('-').collect();
    let numeric = !parts.is_empty()
        && parts.len() <= 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    numeric.then(|| format!("iOS {}", parts.join(".")))
}

/// The process id `simctl launch` printed for the app `bundle`: its line `<bundle>: <pid>`.
#[must_use]
pub fn parse_launched(printed: &str, bundle: &str) -> Option<u32> {
    printed.lines().find_map(|line| {
        let pid = line.trim().strip_prefix(bundle)?.strip_prefix(':')?.trim();
        pid.parse::<u32>().ok().filter(|pid| *pid > 0)
    })
}

/// The failure of a host that names no simulator: the lane asks `xcrun` nothing until one is.
pub(crate) fn unnamed() -> Error {
    unavailable(format!(
        "no simulator is named: set {UDID_ENV} to the UDID of a booted iOS simulator (`xcrun \
         simctl list devices booted`)"
    ))
}
