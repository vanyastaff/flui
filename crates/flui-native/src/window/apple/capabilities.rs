//! What an Apple window honours, and why what it does not is missing: the
//! pure half of the AppKit and UIKit windows, which every host's tests run.
//!
//! Each absence is the platform's, the device's or the host's
//! ([`CapabilityAbsence`]): a feature the OS gained in a version above the
//! floor is gated ([`Since`](crate::version::gate::Since)) and answers
//! `NotOnThisDevice` below it, and `NotYetOnThisHost` from it on while the
//! host does not drive it.

#[cfg(any(target_os = "ios", test))]
use flui_platform::Availability;
use flui_platform::{CapabilityAbsence, WindowCapabilities, WindowCommand, WindowMaterial};

use crate::version::gate::Gate;
#[cfg(any(target_os = "ios", test))]
use crate::version::gate::apple::AppleVersion;
#[cfg(any(target_os = "macos", test))]
use crate::version::gate::apple::MacOSVersion;

/// The command that stands for the window material in the capabilities.
const MATERIAL: WindowCommand = WindowCommand::SetMaterial(WindowMaterial::Opaque);

/// What a scene's window honours: `UIWindow` has no title, frame, position or
/// commands, and observes nothing; the app may only ask the system to destroy
/// its scene session, and the display link takes a refresh-rate request.
/// UIKit draws a material behind a view (`UIVisualEffectView`), which this
/// host does not place yet.
#[cfg(any(target_os = "ios", test))]
pub(crate) const fn scene_capabilities() -> WindowCapabilities {
    let mut capabilities =
        WindowCapabilities::NONE.unavailable(&MATERIAL, CapabilityAbsence::NotYetOnThisHost);
    capabilities.closes = Availability::Native;
    capabilities.frame_rate_hint = Availability::Native;
    capabilities
}

/// `UIPointerInteraction` and `UIPointerStyle` (iOS 13.4): the pointer of a
/// trackpad or mouse on an iPad, whose shape a view chooses. Without them no
/// pointer is shown.
#[cfg(any(target_os = "ios", test))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct PointerInteraction;

#[cfg(any(target_os = "ios", test))]
impl Gate for PointerInteraction {
    type Version = AppleVersion;
    const SINCE: u64 = AppleVersion::new(13, 4, 0).rank();
}

/// Why a UIKit window does not set the pointer's shape. An iPad from iOS 13.4
/// shows a pointer whose style a view chooses, which this host does not set
/// yet; an iPhone shows none, nor does an iPad below 13.4.
#[cfg(any(target_os = "ios", test))]
pub(crate) fn pointer_absence(version: AppleVersion, shows_a_pointer: bool) -> CapabilityAbsence {
    match version.since::<PointerInteraction>() {
        Some(_) if shows_a_pointer => CapabilityAbsence::NotYetOnThisHost,
        Some(_) | None => CapabilityAbsence::NotOnThisDevice,
    }
}

/// What every AppKit window honours and observes, known before one exists
/// (`Platform::window_capabilities`). Each field is named, so a command added
/// to the contract is unavailable here until `apply` carries it out. The
/// material is an `NSVisualEffectView` behind FLUI's view, which lives on the
/// main thread: a window whose owner is not the AppKit main lane draws none,
/// which the host could reach through the main lane and does not yet.
#[cfg(any(target_os = "macos", test))]
pub(crate) const fn appkit_window_capabilities(owner_is_main: bool) -> WindowCapabilities {
    use flui_platform::Availability::Native;
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_title = Native;
    capabilities.sets_position = Native;
    capabilities.sets_size = Native;
    capabilities.sets_constraints = Native;
    capabilities.sets_resizable = Native;
    capabilities.sets_decorated = Native;
    capabilities.minimizes = Native;
    capabilities.maximizes = Native;
    capabilities.enters_fullscreen = Native;
    capabilities.hides = Native;
    capabilities.shows = Native;
    capabilities.activates = Native;
    capabilities.closes = Native;
    capabilities.observes_position = Native;
    capabilities.observes_minimized = Native;
    capabilities.places_at_open = Native;
    if owner_is_main {
        capabilities.material = Native;
    } else {
        capabilities = capabilities.unavailable(&MATERIAL, CapabilityAbsence::NotYetOnThisHost);
    }
    capabilities
}

/// `NSView.displayLink` (macOS 14), whose `preferredFrameRateRange` a
/// refresh-rate request rides on. Without it a window has no per-view display
/// link to ask.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ViewDisplayLink;

#[cfg(any(target_os = "macos", test))]
impl Gate for ViewDisplayLink {
    type Version = MacOSVersion;
    const SINCE: u64 = MacOSVersion::new(14, 0, 0).rank();
}

/// Why an AppKit window takes no refresh-rate request: below macOS 14 there
/// is no per-view display link to carry one; from 14 on there is, and this
/// host paces its frames without it.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn frame_rate_absence(version: MacOSVersion) -> CapabilityAbsence {
    match version.since::<ViewDisplayLink>() {
        Some(_) => CapabilityAbsence::NotYetOnThisHost,
        None => CapabilityAbsence::NotOnThisDevice,
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{Availability, CapabilityAbsence, WindowCapabilities};

    use super::{
        AppleVersion, MATERIAL, MacOSVersion, PointerInteraction, ViewDisplayLink,
        appkit_window_capabilities, frame_rate_absence, pointer_absence, scene_capabilities,
    };
    use crate::version::gate::testing::row;

    /// How a window's capabilities refuse its material.
    fn material(capabilities: &WindowCapabilities) -> Option<CapabilityAbsence> {
        (capabilities.availability(&MATERIAL) == Availability::Unavailable)
            .then(|| capabilities.refusal(&MATERIAL))
    }

    /// Each Apple site, at a version where it matters, and the absence it
    /// answers; `None` where the capability is there.
    #[test]
    fn apple_capability_absences() {
        let ios = AppleVersion::new;
        let macos = MacOSVersion::new;
        let rows: [(&str, Option<CapabilityAbsence>, Option<CapabilityAbsence>); 9] = [
            (
                "UIKit material",
                material(&scene_capabilities()),
                Some(CapabilityAbsence::NotYetOnThisHost),
            ),
            (
                "AppKit material on the main lane",
                material(&appkit_window_capabilities(true)),
                None,
            ),
            (
                "AppKit material off the main lane",
                material(&appkit_window_capabilities(false)),
                Some(CapabilityAbsence::NotYetOnThisHost),
            ),
            (
                "AppKit frame rate on macOS 13",
                Some(frame_rate_absence(macos(13, 6, 0))),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
            (
                "AppKit frame rate on macOS 14",
                Some(frame_rate_absence(macos(14, 0, 0))),
                Some(CapabilityAbsence::NotYetOnThisHost),
            ),
            (
                "AppKit frame rate on an unread macOS",
                Some(frame_rate_absence(MacOSVersion::UNKNOWN)),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
            (
                "UIKit pointer on an iPad, iOS 13.4",
                Some(pointer_absence(ios(13, 4, 0), true)),
                Some(CapabilityAbsence::NotYetOnThisHost),
            ),
            (
                "UIKit pointer on an iPad, iOS 13.3",
                Some(pointer_absence(ios(13, 3, 0), true)),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
            (
                "UIKit pointer on an iPhone, iOS 17",
                Some(pointer_absence(ios(17, 0, 0), false)),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
        ];
        let failures: Vec<String> = rows
            .iter()
            .filter(|(_, got, want)| got != want)
            .map(|(site, got, want)| format!("{site}: {got:?}, expected {want:?}"))
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The gates declared here, beside the table of the other markers
    /// (`version::gate`).
    #[test]
    fn apple_window_gate_table() {
        let mut failures = Vec::new();
        failures.extend(row::<PointerInteraction>(
            "PointerInteraction",
            AppleVersion::new(13, 3, 9),
            AppleVersion::new(13, 4, 0),
            AppleVersion::UNKNOWN,
        ));
        failures.extend(row::<ViewDisplayLink>(
            "ViewDisplayLink",
            MacOSVersion::new(13, 99, 99),
            MacOSVersion::new(14, 0, 0),
            MacOSVersion::UNKNOWN,
        ));
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
