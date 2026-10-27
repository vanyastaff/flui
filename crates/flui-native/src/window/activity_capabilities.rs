//! What an Activity's window honours, and why what it does not is missing:
//! the pure half of the Android window, apart from the JNI reads its facts
//! come from, so every host's tests run it.

use flui_platform::{
    Availability, CapabilityAbsence, WindowCapabilities, WindowCommand, WindowMaterial,
};

use crate::version::gate::android::{AndroidApi, BackgroundBlur, PointerIcons};

/// What the system said that an Activity's window capabilities rest on.
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
pub(crate) fn activity_capabilities(facts: ActivityFacts) -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE.unavailable(
        &WindowCommand::SetMaterial(WindowMaterial::Opaque),
        material_absence(facts.api, facts.cross_window_blur),
    );
    capabilities.frame_rate_hint = facts.frame_rate_hint;
    capabilities
}

/// Why an Activity's window draws no material. From Android 12 (API 31) the
/// system blurs behind a window (`Window.setBackgroundBlurRadius`) where the
/// device blurs across windows, which this host does not ask for yet; below
/// API 31, or where cross-window blur is off, this device has none.
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
pub(crate) fn pointer_icon_absence(api: AndroidApi) -> CapabilityAbsence {
    match api.since::<PointerIcons>() {
        Some(_) => CapabilityAbsence::NotYetOnThisHost,
        None => CapabilityAbsence::NotOnThisDevice,
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{
        Availability, CapabilityAbsence, WindowCapabilities, WindowCommand, WindowMaterial,
    };

    use super::{ActivityFacts, activity_capabilities, pointer_icon_absence};
    use crate::version::gate::android::AndroidApi;

    fn facts(level: u32, cross_window_blur: bool) -> ActivityFacts {
        ActivityFacts {
            api: AndroidApi::from_level(level),
            frame_rate_hint: Availability::Unavailable,
            cross_window_blur,
        }
    }

    /// How a window's capabilities refuse its material.
    fn material(capabilities: &WindowCapabilities) -> Option<CapabilityAbsence> {
        let command = WindowCommand::SetMaterial(WindowMaterial::System);
        (!capabilities.permits(&command)).then(|| capabilities.refusal(&command))
    }

    /// Each Android site at an API level where it matters, and the absence it
    /// answers.
    #[test]
    fn activity_capability_absences() {
        let rows: [(&str, Option<CapabilityAbsence>, Option<CapabilityAbsence>); 7] = [
            (
                "material, API 30",
                material(&activity_capabilities(facts(30, true))),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
            (
                "material, API 31 with cross-window blur",
                material(&activity_capabilities(facts(31, true))),
                Some(CapabilityAbsence::NotYetOnThisHost),
            ),
            (
                "material, API 31 with cross-window blur off",
                material(&activity_capabilities(facts(31, false))),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
            (
                "material, unknown API",
                material(&activity_capabilities(ActivityFacts {
                    api: AndroidApi::UNKNOWN,
                    ..facts(31, true)
                })),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
            (
                "pointer icon, API 23",
                Some(pointer_icon_absence(AndroidApi::from_level(23))),
                Some(CapabilityAbsence::NotOnThisDevice),
            ),
            (
                "pointer icon, API 24",
                Some(pointer_icon_absence(AndroidApi::from_level(24))),
                Some(CapabilityAbsence::NotYetOnThisHost),
            ),
            (
                "pointer icon, unknown API",
                Some(pointer_icon_absence(AndroidApi::UNKNOWN)),
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

    /// The refresh-rate capability is the system's, as read.
    #[test]
    fn the_frame_rate_capability_is_the_systems() {
        for availability in [Availability::Native, Availability::Unavailable] {
            let capabilities = activity_capabilities(ActivityFacts {
                frame_rate_hint: availability,
                ..facts(31, true)
            });
            assert_eq!(capabilities.frame_rate_hint, availability);
        }
    }
}
