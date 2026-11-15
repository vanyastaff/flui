//! What the native windows honour, and why what they do not is missing: the
//! pure rules of the AppKit, UIKit and Activity windows, apart from the OS
//! reads their facts come from, so every host's tests run them.
//!
//! Each absence is the platform's, the device's or the host's
//! ([`CapabilityAbsence`]): a feature the OS gained in a version above the
//! floor is gated ([`Since`](crate::version::gate::Since)) and answers
//! `NotOnThisDevice` below it, and `NotYetOnThisHost` from it on while the
//! host does not drive it.
//!
//! The AppKit window reports when its frames reached the screen: its view is
//! backed by a `CAMetalLayer` whose drawables tell
//! (`owner::apple::metal_layer`), from macOS 10.15.4, below the floor, except
//! on a hypervisor's guest, whose virtual display tells nothing
//! ([`appkit_presentation`]). The UIKit and Activity windows do not yet
//! ([`PRESENTATION`]).

/// Why a UIKit or Activity window reports no presentation, though its
/// platform tells when a frame reached the screen. On iOS that is the
/// drawable's `addPresentedHandler`, which the view's own `CAMetalLayer`
/// would hand out as the AppKit view's does; with such a layer as the view's
/// layer the iOS Simulator showed nothing at all. On Android the presenting
/// swap chain's `VK_GOOGLE_display_timing` is wgpu's to read and it reads
/// none; the choreographer's frame timelines name when a frame is due on
/// screen, which is not that it was.
#[cfg(any(target_os = "ios", target_os = "android", test))]
pub(crate) const PRESENTATION: CapabilityAbsence = CapabilityAbsence::NOT_YET_ON_THIS_HOST;

#[cfg(any(target_os = "ios", target_os = "android", test))]
use flui_platform::Availability;
use flui_platform::{CapabilityAbsence, WindowCapabilities, WindowCapability};

#[cfg(any(target_os = "ios", target_os = "macos", test))]
use crate::version::gate::Gate;
#[cfg(any(target_os = "android", test))]
use crate::version::gate::android::{
    AndroidApi, BackgroundBlur, PointerIcons, SystemGestureInsets,
};
#[cfg(any(target_os = "ios", test))]
use crate::version::gate::apple::AppleVersion;
#[cfg(any(target_os = "macos", test))]
use crate::version::gate::apple::MacOSVersion;

// ---- UIKit -------------------------------------------------------------------

/// What a scene's window honours: `UIWindow` has no title, frame, position or
/// commands, and observes nothing; the app may only ask the system to destroy
/// its scene session, and the display link takes a refresh-rate request.
/// UIKit draws a material behind a view (`UIVisualEffectView`), which this
/// host does not place yet. The window reports the keyboard's occlusion from
/// `UIKeyboardWillChangeFrameNotification`; UIKit names no system gesture
/// insets, only edges an app may defer.
#[cfg(any(target_os = "ios", test))]
pub(crate) const fn scene_capabilities() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.material = WindowCapability::Absent(CapabilityAbsence::NOT_YET_ON_THIS_HOST);
    capabilities.reports_presentation = WindowCapability::Absent(PRESENTATION);
    capabilities.reports_keyboard_inset = WindowCapability::Native;
    capabilities.closes = WindowCapability::Native;
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
        Some(_) if shows_a_pointer => CapabilityAbsence::NOT_YET_ON_THIS_HOST,
        Some(_) | None => CapabilityAbsence::NOT_ON_THIS_DEVICE,
    }
}

// ---- AppKit ------------------------------------------------------------------

/// What every AppKit window honours and observes, known before one exists
/// (`Platform::window_capabilities`). Each field is named, so a command added
/// to the contract is unavailable here until `apply` carries it out. The
/// material is an `NSVisualEffectView` behind FLUI's view, on the main thread
/// every AppKit window is made on (`MacOSWindow::new_inner` takes its
/// `MainThreadMarker`). The
/// window reports when its frames reached the screen unless `presentation`
/// names why the machine's display cannot tell ([`appkit_presentation`]).
#[cfg(any(target_os = "macos", test))]
pub(crate) const fn appkit_window_capabilities(
    presentation: Option<CapabilityAbsence>,
) -> WindowCapabilities {
    use WindowCapability::{Absent, Native};
    use flui_platform::Availability;
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.reports_presentation = match presentation {
        Some(absence) => Absent(absence),
        None => Native,
    };
    capabilities.sets_title = Native;
    capabilities.sets_position = Native;
    capabilities.sets_size = Native;
    capabilities.sets_constraints = Native;
    capabilities.sets_resizable = Native;
    capabilities.sets_frame = Native;
    capabilities.minimizes = Native;
    capabilities.maximizes = Native;
    capabilities.enters_fullscreen = Native;
    capabilities.hides = Native;
    capabilities.shows = Native;
    capabilities.activates = Native;
    capabilities.closes = Native;
    capabilities.observes_position = Availability::Native;
    capabilities.observes_minimized = Availability::Native;
    capabilities.places_at_open = Availability::Native;
    // A title bar of the application's is drawn under the system's, made
    // transparent (`fullSizeContentView`), with the traffic lights over it.
    capabilities.app_title_bar = Native;
    capabilities.material = Native;
    capabilities
}

/// Why an AppKit window reports no presentation on this machine; `None` where
/// its display tells when a drawable reached the screen.
///
/// A Mac that is a hypervisor's guest (`kern.hv_vmm_present`) shows its
/// windows on a virtual display: on a `macos-latest` runner (Apple Virtual
/// display at 60 Hz, Metal's Apple Paravirtual device) every drawable a
/// visible, unoccluded window presented was handed to its presented handler
/// with a `presentedTime` of 0, Metal's "never shown", so no frame is ever
/// reported.
///
/// The guest flag is a proxy: the cause is the virtual display, and a guest
/// whose display does stamp its drawables is told it reports nothing. It is
/// used because the answer is owed before any frame
/// (`Platform::window_capabilities` before a window exists, the snapshot at
/// open), and nothing readable then names the cause: Metal says whether a
/// drawable was shown only after it was presented, and the virtual display is
/// told apart from a real one only by a localized screen name or an EDID
/// vendor of 0, which a real display without EDID shares. Downgrading after
/// the first unstamped drawables would leave the claim untrue for those
/// frames and change a capability under a running window.
#[cfg(any(target_os = "macos", test))]
pub(crate) const fn appkit_presentation(hypervisor_guest: bool) -> Option<CapabilityAbsence> {
    if hypervisor_guest {
        Some(CapabilityAbsence::NOT_ON_THIS_DEVICE)
    } else {
        None
    }
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
/// host paces its frames without it. winit's macOS backend carries none either, and
/// answers the same.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn frame_rate_absence(version: MacOSVersion) -> CapabilityAbsence {
    match version.since::<ViewDisplayLink>() {
        Some(_) => CapabilityAbsence::NOT_YET_ON_THIS_HOST,
        None => CapabilityAbsence::NOT_ON_THIS_DEVICE,
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
/// [`material_absence`] gives. The window reports the keyboard's occlusion,
/// and the system gesture regions from API 29.
#[cfg(any(target_os = "android", test))]
pub(crate) fn activity_capabilities(facts: ActivityFacts) -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.material =
        WindowCapability::Absent(material_absence(facts.api, facts.cross_window_blur));
    capabilities.reports_presentation = WindowCapability::Absent(PRESENTATION);
    capabilities.reports_keyboard_inset = WindowCapability::Native;
    capabilities.reports_system_gestures = match facts.api.since::<SystemGestureInsets>() {
        Some(_) => WindowCapability::Native,
        None => WindowCapability::Absent(CapabilityAbsence::NOT_ON_THIS_DEVICE),
    };
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
        CapabilityAbsence::NOT_YET_ON_THIS_HOST
    } else {
        CapabilityAbsence::NOT_ON_THIS_DEVICE
    }
}

/// Why an Activity's window does not set the pointer's icon. From API 24 a
/// view shows a `PointerIcon` for a mouse or stylus, which this host does not
/// set yet; below it the system draws its own pointer.
#[cfg(any(target_os = "android", test))]
pub(crate) fn pointer_icon_absence(api: AndroidApi) -> CapabilityAbsence {
    match api.since::<PointerIcons>() {
        Some(_) => CapabilityAbsence::NOT_YET_ON_THIS_HOST,
        None => CapabilityAbsence::NOT_ON_THIS_DEVICE,
    }
}

/// A capability read the two ways a test compares it: how it is delivered,
/// and why it is missing.
#[cfg(test)]
pub(crate) trait CapabilityReading {
    /// How the capability is delivered, without the reason it is not.
    fn availability(self) -> flui_platform::Availability;
    /// Why the window lacks the capability; `None` while it has it.
    fn absence(self) -> Option<CapabilityAbsence>;
}

#[cfg(test)]
impl CapabilityReading for WindowCapability {
    fn availability(self) -> flui_platform::Availability {
        match self {
            Self::Native => flui_platform::Availability::Native,
            Self::Emulated => flui_platform::Availability::Emulated,
            Self::Absent(_) => flui_platform::Availability::Unavailable,
        }
    }

    fn absence(self) -> Option<CapabilityAbsence> {
        match self {
            Self::Absent(absence) => Some(absence),
            Self::Native | Self::Emulated => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{Availability, CapabilityAbsence};

    use super::CapabilityReading as _;

    use super::{
        ActivityFacts, AppleVersion, MacOSVersion, PointerInteraction, ViewDisplayLink,
        activity_capabilities, appkit_presentation, appkit_window_capabilities, frame_rate_absence,
        pointer_absence, pointer_icon_absence, scene_capabilities,
    };
    use crate::version::gate::android::AndroidApi;
    use crate::version::gate::testing::row;

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
        const DEVICE: CapabilityAbsence = CapabilityAbsence::NOT_ON_THIS_DEVICE;
        const HOST: CapabilityAbsence = CapabilityAbsence::NOT_YET_ON_THIS_HOST;
        let ios = AppleVersion::new;
        let macos = MacOSVersion::new;
        let api = AndroidApi::from_level;
        let rows: [(&str, Option<CapabilityAbsence>, Option<CapabilityAbsence>); 24] = [
            (
                "UIKit material",
                scene_capabilities().material.absence(),
                Some(HOST),
            ),
            (
                "UIKit pointer on an iPad, iOS 13.4",
                Some(pointer_absence(ios(13, 4, 0), true)),
                Some(HOST),
            ),
            (
                "UIKit pointer on an iPad, iOS 13.3",
                Some(pointer_absence(ios(13, 3, 0), true)),
                Some(DEVICE),
            ),
            (
                "UIKit pointer on an iPhone, iOS 17",
                Some(pointer_absence(ios(17, 0, 0), false)),
                Some(DEVICE),
            ),
            (
                "AppKit material",
                appkit_window_capabilities(None).material.absence(),
                None,
            ),
            (
                "AppKit title bar of the application's",
                appkit_window_capabilities(None).app_title_bar.absence(),
                None,
            ),
            (
                "AppKit frame rate on macOS 13",
                Some(frame_rate_absence(macos(13, 6, 0))),
                Some(DEVICE),
            ),
            (
                "AppKit frame rate on macOS 14",
                Some(frame_rate_absence(macos(14, 0, 0))),
                Some(HOST),
            ),
            (
                "AppKit frame rate on an unread macOS",
                Some(frame_rate_absence(MacOSVersion::UNKNOWN)),
                Some(DEVICE),
            ),
            (
                "Activity material, API 30",
                activity_capabilities(activity(30, true)).material.absence(),
                Some(DEVICE),
            ),
            (
                "Activity material, API 31 with cross-window blur",
                activity_capabilities(activity(31, true)).material.absence(),
                Some(HOST),
            ),
            (
                "UIKit keyboard inset",
                scene_capabilities().reports_keyboard_inset.absence(),
                None,
            ),
            (
                "UIKit presentation report",
                scene_capabilities().reports_presentation.absence(),
                Some(HOST),
            ),
            (
                "AppKit presentation report on a Mac",
                appkit_window_capabilities(appkit_presentation(false))
                    .reports_presentation
                    .absence(),
                None,
            ),
            (
                "AppKit presentation report on a hypervisor's guest",
                appkit_window_capabilities(appkit_presentation(true))
                    .reports_presentation
                    .absence(),
                Some(DEVICE),
            ),
            (
                "Activity keyboard inset, API 23",
                activity_capabilities(activity(23, false))
                    .reports_keyboard_inset
                    .absence(),
                None,
            ),
            (
                "Activity system gestures, API 28",
                activity_capabilities(activity(28, false))
                    .reports_system_gestures
                    .absence(),
                Some(DEVICE),
            ),
            (
                "Activity system gestures, API 29",
                activity_capabilities(activity(29, false))
                    .reports_system_gestures
                    .absence(),
                None,
            ),
            (
                "Activity presentation report",
                activity_capabilities(activity(33, true))
                    .reports_presentation
                    .absence(),
                Some(HOST),
            ),
            (
                "Activity material, API 31 with cross-window blur off",
                activity_capabilities(activity(31, false))
                    .material
                    .absence(),
                Some(DEVICE),
            ),
            (
                "Activity material, unknown API",
                activity_capabilities(ActivityFacts {
                    api: AndroidApi::UNKNOWN,
                    ..activity(31, true)
                })
                .material
                .absence(),
                Some(DEVICE),
            ),
            (
                "Activity pointer icon, API 23",
                Some(pointer_icon_absence(api(23))),
                Some(DEVICE),
            ),
            (
                "Activity pointer icon, API 24",
                Some(pointer_icon_absence(api(24))),
                Some(HOST),
            ),
            (
                "Activity pointer icon, unknown API",
                Some(pointer_icon_absence(AndroidApi::UNKNOWN)),
                Some(DEVICE),
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
