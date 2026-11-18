//! FLUI's OS shell services, the first capability crate (ADR-0154 §11).
//!
//! - **Launching**: open a URI, a file or a folder in the system's handler,
//!   show a file in the file manager, move it to the trash. The handle,
//!   [`flui_platform::launch::Launcher`], lives in
//!   the contract crate, since framework widgets name it (ADR-0154 §3);
//!   this crate provides it.
//! - **File dialogs**: [`dialog::FileDialogs`], open and save dialogs modal to
//!   the acquiring widget's window.
//! - **Autostart**: [`autostart::Autostart`], starting the application when
//!   the user logs in.
//! - **Global shortcuts**: [`shortcuts::GlobalShortcuts`], shortcuts that
//!   reach the application while another one has the focus.
//! - **The launcher entry**: [`taskbar::TaskbarEntry`], progress and a count
//!   on the application's dock or taskbar entry.
//! - **User folders**: [`user_dirs::UserDirs`], where the user keeps
//!   documents, downloads and the rest.
//!
//! An application registers [`SystemPlugin`]; a scenario registers
//! `testing::ScriptedSystem` instead (feature `testing`).
//!
//! Linux reaches them through the XDG desktop portal on the session bus,
//! the launcher entry through its session-bus signal, and the user folders
//! through the XDG user directories. Elsewhere, until a platform's provider
//! lands, the handles are refused with `UnsupportedReason::NotYetOnThisHost`,
//! never faked (ADR-0154 §10).

pub mod autostart;
pub mod dialog;
#[cfg(target_os = "linux")]
mod portal;
pub mod shortcuts;
pub mod taskbar;
#[cfg(feature = "testing")]
pub mod testing;
pub mod user_dirs;

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
        portal::register(registrar, None, None);
        #[cfg(not(target_os = "linux"))]
        not_yet::register(registrar);
    }
}

/// The refusals of a platform with no provider yet.
#[cfg(not(target_os = "linux"))]
mod not_yet {
    use flui_platform::capability::{Capability, CapabilityRegistrar, ProviderContext};
    use flui_platform::launch::LauncherCapability;
    use flui_platform::{Unsupported, UnsupportedReason};

    use crate::autostart::AutostartCapability;
    use crate::dialog::FileDialogCapability;
    use crate::shortcuts::GlobalShortcutCapability;
    use crate::taskbar::TaskbarCapability;
    use crate::user_dirs::UserDirCapability;

    /// Provide `C` as a refusal: this host has no provider for it yet.
    fn refuse<C: Capability>(registrar: &mut CapabilityRegistrar) {
        registrar.provide::<C>(|_: &ProviderContext<'_>| {
            Err::<C::Handle, _>(Unsupported::of::<C>(UnsupportedReason::NotYetOnThisHost))
        });
    }

    pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
        refuse::<LauncherCapability>(registrar);
        refuse::<FileDialogCapability>(registrar);
        refuse::<AutostartCapability>(registrar);
        refuse::<GlobalShortcutCapability>(registrar);
        refuse::<TaskbarCapability>(registrar);
        refuse::<UserDirCapability>(registrar);
    }
}
