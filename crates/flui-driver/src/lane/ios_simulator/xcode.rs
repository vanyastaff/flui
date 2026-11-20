//! The Xcode the Simulator runs under, and the input the lane refuses there.
//!
//! The lane reads the selected Xcode's version (`xcodebuild -version`, [`parse_version`]) once,
//! as it connects, and names it in every refusal of input ([`hid_refusal`]): the channels into a
//! simulator's input change with the Xcode release, so the version is what a reader needs to judge
//! a refusal.
//!
//! **Why no input.** No public channel puts a touch, a key or a hardware button into a simulator
//! from the host: `simctl io` captures and records the screen and sends nothing. The channels
//! other tools use are private, and none can be held to the driver's rules:
//!
//! - `SimulatorKit`'s Indigo HID messages (the `SimDeviceLegacyHIDClient` that `idb` used):
//!   reverse-engineered C structures and functions, whose layout a selector probe cannot check,
//!   so a mismatch is memory sent wrong rather than a refusal. From CoreSimulator 1155.4 (Xcode 27)
//!   the Simulator drops Indigo button and keyboard events on every runtime and delivers its touch
//!   only some of the time, while the send still reports success (`idb`'s `SimulatorIndigoHIDTransport`).
//! - The per-simulator DTUHID service that replaced it there: a private XPC protocol to a daemon
//!   inside the simulator that aborts while a boot settles and then discards every send in silence,
//!   which `idb` meets with liveness probes, retries and timed drains (its
//!   `SimulatorDTUHIDConnection`). A send cannot tell a delivered event from a dropped one.
//! - Quartz events on the Simulator's own window, through the macOS lane: public calls, but they
//!   land on the shared desktop (the simulator is no longer the isolation it is for), the window's
//!   layout around the screen (bezels, zoom) is published by no API, and the mouse stands in for
//!   one finger.
//!
//! So every input event is refused as `not_supported` naming [`HID_CAPABILITY`], before anything
//! is sent; capture, the app's process and the agent's own trees and synthetic input still work.

use crate::error::Error;

/// The capability a refused input names.
pub const HID_CAPABILITY: &str = "ios_simulator_hid";

/// The selected Xcode, as `xcodebuild -version` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct XcodeVersion {
    /// Its version: `27.1`.
    pub version: String,
    /// Its build: `27A9269`, when printed.
    pub build: Option<String>,
}

impl std::fmt::Display for XcodeVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Xcode {}", self.version)?;
        if let Some(build) = &self.build {
            write!(f, " ({build})")?;
        }
        Ok(())
    }
}

/// The version `printed` names: its line `Xcode <version>`, a version of one to three numbers,
/// and its line `Build version <build>` when there is one, a build of ASCII letters and digits.
#[must_use]
pub fn parse_version(printed: &str) -> Option<XcodeVersion> {
    let mut lines = printed.lines().map(str::trim);
    let version = lines.clone().find_map(|line| line.strip_prefix("Xcode "))?;
    let parts: Vec<&str> = version.split('.').collect();
    let numeric = parts.len() <= 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !numeric {
        return None;
    }
    let build = lines
        .find_map(|line| line.strip_prefix("Build version "))
        .filter(|build| {
            !build.is_empty()
                && build.len() <= 32
                && build.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .map(str::to_owned);
    Some(XcodeVersion {
        version: version.to_owned(),
        build,
    })
}

/// The refusal of input into a simulator running `runtime` under `xcode` (`None` when its version
/// could not be read): see the [module docs](self).
#[must_use]
pub fn hid_refusal(xcode: Option<&XcodeVersion>, runtime: &str) -> Error {
    let xcode = xcode.map_or_else(
        || "an Xcode whose version `xcodebuild -version` did not give".to_owned(),
        ToString::to_string,
    );
    Error::not_supported(
        HID_CAPABILITY,
        format!(
            "the iOS Simulator lane sends no touch, key or button into {runtime} under {xcode}: no \
             public channel reaches a simulator's input from the host, and the private ones \
             (Indigo HID, which drops buttons and keys from Xcode 27, and the DTUHID service) \
             cannot tell a delivered event from a dropped one; use the synthetic path"
        ),
    )
}
