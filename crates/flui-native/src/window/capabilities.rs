//! What the native windows honour, and why what they do not is missing: the
//! pure rules of the AppKit, UIKit and Activity windows, apart from the OS
//! reads their facts come from, so every host's tests run them.
//!
//! Each absence is the platform's, the device's or the host's
//! ([`CapabilityAbsence`]): a feature the OS gained in a version above the
//! floor is gated ([`Since`](crate::version::gate::Since)) and answers
//! `NotOnThisDevice` below it, and `NotYetOnThisHost` from it on while the
//! host does not drive it.

#[cfg(any(target_os = "ios", target_os = "android", test))]
use flui_platform::Availability;
use flui_platform::{CapabilityAbsence, WindowCapabilities, WindowCommand, WindowMaterial};

#[cfg(any(target_os = "ios", target_os = "macos", test))]
use crate::version::gate::Gate;
#[cfg(any(target_os = "android", test))]
use crate::version::gate::android::{AndroidApi, BackgroundBlur, PointerIcons};
#[cfg(any(target_os = "ios", test))]
use crate::version::gate::apple::AppleVersion;
#[cfg(any(target_os = "macos", test))]
use crate::version::gate::apple::MacOSVersion;

/// The command that stands for the window material in the capabilities.
const MATERIAL: WindowCommand = WindowCommand::SetMaterial(WindowMaterial::Opaque);

// ---- UIKit -------------------------------------------------------------------

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

// ---- AppKit ------------------------------------------------------------------

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
/// host paces its frames without it. winit carries none on macOS either, and
/// answers the same.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn frame_rate_absence(version: MacOSVersion) -> CapabilityAbsence {
    match version.since::<ViewDisplayLink>() {
        Some(_) => CapabilityAbsence::NotYetOnThisHost,
        None => CapabilityAbsence::NotOnThisDevice,
    }
}

// ---- Activity ----------------------------------------------------------------

/// What the system said that an Activity's window capabilities rest on.
#[cfg(any(target_os = "android", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActivityFacts {
    /// The running API level.
    pub(crate) api: AndroidApi,
    /// Whether the system takes a refresh-rate request
    /// (`ANativeWindow_setFrameRate`, API 30).
    pub(crate) frame_rate_hint: Availability,
    /// `WindowManager.isCrossWindowBlurEnabled`: the device blurs behind
    /// windows now (off on hardware without it, in battery saver, with the
    /// developer setting off). Read only from API 31.
    pub(crate) cross_window_blur: bool,
}

/// What an Activity's window honours: no command, as the system owns the
/// surface and the Activity's lifetime, and a refresh-rate request where the
/// system takes one. The material is missing for the reason
/// [`material_absence`] gives.
#[cfg(any(target_os = "android", test))]
pub(crate) fn activity_capabilities(facts: ActivityFacts) -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE.unavailable(
        &MATERIAL,
        material_absence(facts.api, facts.cross_window_blur),
    );
    capabilities.frame_rate_hint = facts.frame_rate_hint;
    capabilities
}

/// Why an Activity's window draws no material. From Android 12 (API 31) the
/// system blurs behind a window (`Window.setBackgroundBlurRadius`) where the
/// device blurs across windows, which this host does not ask for yet; below
/// API 31, or where cross-window blur is off, this device has none.
#[cfg(any(target_os = "android", test))]
pub(crate) fn material_absence(api: AndroidApi, cross_window_blur: bool) -> CapabilityAbsence {
    if api.since::<BackgroundBlur>().is_some() && cross_window_blur {
        CapabilityAbsence::NotYetOnThisHost
    } else {
        CapabilityAbsence::NotOnThisDevice
    }
}

/// Why an Activity's window does not set the pointer's icon. From API 24 a
/// view shows a `PointerIcon` for a mouse or stylus, which this host does not
/// set yet; below it the system draws its own pointer.
#[cfg(any(target_os = "android", test))]
pub(crate) fn pointer_icon_absence(api: AndroidApi) -> CapabilityAbsence {
    match api.since::<PointerIcons>() {
        Some(_) => CapabilityAbsence::NotYetOnThisHost,
        None => CapabilityAbsence::NotOnThisDevice,
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{Availability, CapabilityAbsence, WindowCapabilities};

    use super::{
        ActivityFacts, AppleVersion, MATERIAL, MacOSVersion, PointerInteraction, ViewDisplayLink,
        activity_capabilities, appkit_window_capabilities, frame_rate_absence, pointer_absence,
        pointer_icon_absence, scene_capabilities,
    };
    use crate::version::gate::android::AndroidApi;
    use crate::version::gate::testing::row;

    /// How a window's capabilities refuse its material; `None` where it draws
    /// one.
    fn material(capabilities: &WindowCapabilities) -> Option<CapabilityAbsence> {
        (capabilities.availability(&MATERIAL) == Availability::Unavailable)
            .then(|| capabilities.refusal(&MATERIAL))
    }

    fn activity(level: u32, cross_window_blur: bool) -> ActivityFacts {
        ActivityFacts {
            api: AndroidApi::from_level(level),
            frame_rate_hint: Availability::Unavailable,
            cross_window_blur,
        }
    }

    /// Each site, at a version where it matters, and the absence it answers;
    /// `None` where the capability is there.
    #[test]
    fn native_capability_absences() {
        use CapabilityAbsence::{NotOnThisDevice, NotYetOnThisHost};
        let ios = AppleVersion::new;
        let macos = MacOSVersion::new;
        let api = AndroidApi::from_level;
        let rows: [(&str, Option<CapabilityAbsence>, Option<CapabilityAbsence>); 16] = [
            (
                "UIKit material",
                material(&scene_capabilities()),
                Some(NotYetOnThisHost),
            ),
            (
                "UIKit pointer on an iPad, iOS 13.4",
                Some(pointer_absence(ios(13, 4, 0), true)),
                Some(NotYetOnThisHost),
            ),
            (
                "UIKit pointer on an iPad, iOS 13.3",
                Some(pointer_absence(ios(13, 3, 0), true)),
                Some(NotOnThisDevice),
            ),
            (
                "UIKit pointer on an iPhone, iOS 17",
                Some(pointer_absence(ios(17, 0, 0), false)),
                Some(NotOnThisDevice),
            ),
            (
                "AppKit material on the main lane",
                material(&appkit_window_capabilities(true)),
                None,
            ),
            (
                "AppKit material off the main lane",
                material(&appkit_window_capabilities(false)),
                Some(NotYetOnThisHost),
            ),
            (
                "AppKit frame rate on macOS 13",
                Some(frame_rate_absence(macos(13, 6, 0))),
                Some(NotOnThisDevice),
            ),
            (
                "AppKit frame rate on macOS 14",
                Some(frame_rate_absence(macos(14, 0, 0))),
                Some(NotYetOnThisHost),
            ),
            (
                "AppKit frame rate on an unread macOS",
                Some(frame_rate_absence(MacOSVersion::UNKNOWN)),
                Some(NotOnThisDevice),
            ),
            (
                "Activity material, API 30",
                material(&activity_capabilities(activity(30, true))),
                Some(NotOnThisDevice),
            ),
            (
                "Activity material, API 31 with cross-window blur",
                material(&activity_capabilities(activity(31, true))),
                Some(NotYetOnThisHost),
            ),
            (
                "Activity material, API 31 with cross-window blur off",
                material(&activity_capabilities(activity(31, false))),
                Some(NotOnThisDevice),
            ),
            (
                "Activity material, unknown API",
                material(&activity_capabilities(ActivityFacts {
                    api: AndroidApi::UNKNOWN,
                    ..activity(31, true)
                })),
                Some(NotOnThisDevice),
            ),
            (
                "Activity pointer icon, API 23",
                Some(pointer_icon_absence(api(23))),
                Some(NotOnThisDevice),
            ),
            (
                "Activity pointer icon, API 24",
                Some(pointer_icon_absence(api(24))),
                Some(NotYetOnThisHost),
            ),
            (
                "Activity pointer icon, unknown API",
                Some(pointer_icon_absence(AndroidApi::UNKNOWN)),
                Some(NotOnThisDevice),
            ),
        ];
        let failures: Vec<String> = rows
            .iter()
            .filter(|(_, got, want)| got != want)
            .map(|(site, got, want)| format!("{site}: {got:?}, expected {want:?}"))
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The Activity's refresh-rate capability is the system's, as read.
    #[test]
    fn the_activity_frame_rate_capability_is_the_systems() {
        for availability in [Availability::Native, Availability::Unavailable] {
            let capabilities = activity_capabilities(ActivityFacts {
                frame_rate_hint: availability,
                ..activity(31, true)
            });
            assert_eq!(capabilities.frame_rate_hint, availability);
        }
    }

    /// The gates declared here, beside the table of the other markers
    /// (`version::gate`).
    #[test]
    fn native_window_gate_table() {
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
