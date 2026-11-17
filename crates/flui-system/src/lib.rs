//! FLUI's OS shell services, the first capability crate (ADR-0154 §11).
//!
//! - **Launching**: open a URI, a file or a folder in the system's handler,
//!   show a file in the file manager, move it to the trash. The handle,
//!   [`flui_platform::launch::Launcher`], lives in the contract crate, since
//!   framework widgets name it (ADR-0154 §3); this crate provides it.
//! - **Dialogs**: [`dialog::FileDialogs`] and [`dialog::MessageBoxes`],
//!   modal to the acquiring widget's window, one dialog per window.
//! - **Sharing**: [`share::Share`], the system's share sheet.
//! - **Launch at login**: [`login::LaunchAtLogin`].
//! - **Global shortcuts**: [`shortcut::GlobalShortcuts`].
//! - **Taskbar**: [`taskbar::Taskbar`], a window's progress and badge.
//! - **Jump list**: [`jump_list::JumpList`], Windows only.
//! - **The OS**: [`os::Os`], the user's folders and what the system is.
//!
//! An application registers [`SystemPlugin`]; a scenario registers
//! `testing::ScriptedSystem` instead (feature `testing`).
//!
//! Windows provides every service. Linux reaches the launcher and the file
//! dialogs through the XDG desktop portal on the session bus. Elsewhere, and
//! for the services a platform's provider does not have yet, the handle is
//! refused with `UnsupportedReason::NotYetOnThisHost`, never faked
//! (ADR-0154 §10); the jump list is refused with `NotOnThisPlatform` off
//! Windows.

pub mod dialog;
pub mod jump_list;
pub mod login;
pub mod os;
#[cfg(target_os = "linux")]
mod portal;
pub mod share;
pub mod shortcut;
pub mod taskbar;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(windows)]
mod win32;

use flui_platform::capability::{CapabilityRegistrar, Plugin};
#[cfg(not(windows))]
use flui_platform::{
    UnsupportedReason,
    capability::{Capability, ProviderContext},
};

/// The system services' providers for the platform the application runs on:
/// the Windows shell on Windows, the XDG desktop portal on Linux, and a
/// refusal for what a platform's provider does not have yet.
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
        #[cfg(windows)]
        win32::register(registrar);
        #[cfg(target_os = "linux")]
        portal::register(registrar, None);
        #[cfg(not(windows))]
        refuse_elsewhere(registrar);
    }
}

/// Register a refusal of `C` for `reason`, a provider that never fakes the
/// service.
#[cfg(not(windows))]
fn refuse<C: Capability>(registrar: &mut CapabilityRegistrar, reason: UnsupportedReason) {
    registrar.provide::<C>(move |_: &ProviderContext<'_>| {
        Err::<C::Handle, _>(flui_platform::Unsupported::of::<C>(reason))
    });
}

/// The refusals off Windows: the services no provider of this platform has
/// yet, and the jump list, which only Windows has.
#[cfg(not(windows))]
fn refuse_elsewhere(registrar: &mut CapabilityRegistrar) {
    let not_yet = UnsupportedReason::NotYetOnThisHost;
    #[cfg(not(target_os = "linux"))]
    {
        refuse::<flui_platform::launch::LauncherCapability>(registrar, not_yet);
        refuse::<dialog::FileDialogCapability>(registrar, not_yet);
    }
    refuse::<dialog::MessageBoxCapability>(registrar, not_yet);
    refuse::<share::ShareCapability>(registrar, not_yet);
    refuse::<login::LaunchAtLoginCapability>(registrar, not_yet);
    refuse::<shortcut::GlobalShortcutCapability>(registrar, not_yet);
    refuse::<taskbar::TaskbarCapability>(registrar, not_yet);
    refuse::<os::OsCapability>(registrar, not_yet);
    refuse::<jump_list::JumpListCapability>(registrar, UnsupportedReason::NotOnThisPlatform);
}
