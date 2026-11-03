//! Linux's floor beyond the C library (ADR-0244 §3): a Wayland compositor
//! that offers `xdg_wm_base`, or an X server with RandR 1.5.
//!
//! The native hosts check it on the connection they open: the Wayland host
//! refuses a compositor whose `xdg_wm_base` it cannot bind, and the X11 host
//! reads RandR's answer through [`randr_version`].

use crate::PlatformError;

/// The requirement a compositor without `xdg_wm_base` lacks.
#[cfg(wayland_host)]
pub(crate) const XDG_WM_BASE: &str = "the Wayland `xdg_wm_base` global";

/// The requirement an X server without RandR 1.5 lacks.
#[cfg(any(x11_host, test))]
pub(crate) const RANDR_1_5: &str = "the X11 RandR 1.5 extension";

/// Linux lacks `requirement`.
pub(crate) fn missing(requirement: &'static str) -> PlatformError {
    PlatformError::MissingRequirement {
        environment: flui_platform::Environment::Linux,
        requirement,
    }
}

/// An X server whose RandR answered `version` (`None`: it has no RandR)
/// meets the floor at 1.5 or later.
#[cfg(any(x11_host, test))]
pub(crate) fn randr_version(version: Option<(u32, u32)>) -> Result<(), PlatformError> {
    if version.is_some_and(|version| version >= (1, 5)) {
        Ok(())
    } else {
        Err(missing(RANDR_1_5))
    }
}

#[cfg(test)]
mod tests {
    use super::{RANDR_1_5, randr_version};
    use crate::PlatformError;

    /// The requirement a result names, `None` when it meets the floor.
    fn lacks(result: Result<(), PlatformError>) -> Option<&'static str> {
        match result {
            Ok(()) => None,
            Err(PlatformError::MissingRequirement {
                environment: flui_platform::Environment::Linux,
                requirement,
            }) => Some(requirement),
            Err(other) => panic!("not a missing requirement: {other}"),
        }
    }

    /// RandR at 1.5 or later meets the floor; anything less names what is
    /// missing.
    #[test]
    fn linux_floor_requirements() {
        let x11 = [
            (Some((1_u32, 5_u32)), None::<&str>),
            (Some((1, 6)), None),
            (Some((2, 0)), None),
            (Some((1, 4)), Some(RANDR_1_5)),
            (Some((0, 9)), Some(RANDR_1_5)),
            (None, Some(RANDR_1_5)),
        ];
        let mut failures = Vec::new();
        for (version, expected) in &x11 {
            let found = lacks(randr_version(*version));
            if found != *expected {
                failures.push(format!("RandR {version:?}: {found:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
