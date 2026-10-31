//! FLUI's OS shell services, the first capability crate (ADR-0154 §11).
//!
//! - **Launching**: open a URI, a file or a folder in the system's handler,
//!   show a file in the file manager, move it to the trash. The handle,
//!   [`flui_platform::launch::Launcher`], lives in
//!   the contract crate, since framework widgets name it (ADR-0154 §3);
//!   this crate provides it.
//! - **File dialogs**: [`dialog::FileDialogs`], open and save dialogs modal to
//!   the acquiring widget's window.
//!
//! An application registers [`SystemPlugin`]; a scenario registers
//! `testing::ScriptedSystem` instead (feature `testing`).
//!
//! Linux reaches both through the XDG desktop portal on the session bus.
//! Elsewhere, until a platform's provider lands, the handles are refused
//! with `UnsupportedReason::NotYetOnThisHost`, never faked (ADR-0154 §10).

pub mod dialog;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(feature = "testing")]
pub mod testing;

use flui_platform::capability::{CapabilityRegistrar, Plugin};

/// The system services' providers for the platform the application runs on:
/// the XDG desktop portal on Linux; elsewhere, until a platform's provider
/// lands, a refusal.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct SystemPlugin;

impl SystemPlugin {
    /// The plugin for the current platform.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for SystemPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        #[cfg(target_os = "linux")]
        portal::register(registrar, None);
        #[cfg(not(target_os = "linux"))]
        not_yet::register(registrar);
    }
}

/// The refusals of a platform with no provider yet.
#[cfg(not(target_os = "linux"))]
mod not_yet {
    use flui_platform::capability::{CapabilityRegistrar, ProviderContext};
    use flui_platform::launch::{Launcher, LauncherCapability};
    use flui_platform::{Unsupported, UnsupportedReason};

    use crate::dialog::{FileDialogCapability, FileDialogs};

    pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
        registrar.provide::<LauncherCapability>(|_: &ProviderContext<'_>| {
            Err::<Launcher, _>(Unsupported::of::<LauncherCapability>(
                UnsupportedReason::NotYetOnThisHost,
            ))
        });
        registrar.provide::<FileDialogCapability>(|_: &ProviderContext<'_>| {
            Err::<FileDialogs, _>(Unsupported::of::<FileDialogCapability>(
                UnsupportedReason::NotYetOnThisHost,
            ))
        });
    }
}
