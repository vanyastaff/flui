//! FLUI's OS shell services, the first capability crate (ADR-0154 §11).
//!
//! - **Launching**: open a URI, a file or a folder in the system's handler,
//!   show a file in the file manager, move it to the trash. The handle,
//!   [`flui_platform::launch::Launcher`], lives in the contract crate, since
//!   framework widgets name it (ADR-0154 §3); this crate provides it.
//! - **Dialogs**: [`dialog::FileDialogs`] and [`dialog::MessageBoxes`],
//!   modal to the acquiring widget's window, one dialog per window.
//! - **Sharing**: [`share::Share`], the system's share sheet.
//! - **Autostart**: [`autostart::Autostart`], starting the application when
//!   the user logs in.
//! - **Global shortcuts**: [`shortcuts::GlobalShortcuts`], shortcuts that
//!   reach the application while another one has the focus.
//! - **The launcher entry**: [`taskbar::TaskbarEntry`], progress and a count
//!   on the application's dock or taskbar entry.
//! - **User folders**: [`user_dirs::UserDirs`], where the user keeps
//!   documents, downloads and the rest.
//! - **Jump list**: [`jump_list::JumpList`], Windows only.
//! - **The OS**: [`os::Os`], what the system is.
//!
//! An application registers [`SystemPlugin`]; a scenario registers
//! `testing::ScriptedSystem` instead (feature `testing`).
//!
//! Windows provides every service, macOS every one but the jump list.
//! Linux reaches the launcher, the file
//! dialogs, autostart and the global shortcuts through the XDG desktop
//! portal on the session bus, the launcher entry through its session-bus
//! signal, and the user folders through the XDG user directories; it
//! refuses the message box, the share sheet and the jump list with
//! `UnsupportedReason::NotOnThisPlatform`, since no portal has them. A web
//! page launches URIs, picks and saves files, shows a message box, shares,
//! and badges an installed application's icon through the browser, and
//! refuses autostart, global shortcuts, the user folders, the jump list and
//! the system information with `NotOnThisPlatform`: a page has none of
//! them. Elsewhere, and for the system information on Linux, the handle is
//! refused with `UnsupportedReason::NotYetOnThisHost` until a platform's
//! provider lands, never faked (ADR-0154 §10); the jump list is refused with
//! `NotOnThisPlatform` everywhere but Windows.

pub mod autostart;
pub mod dialog;
pub mod jump_list;
#[cfg(any(target_os = "macos", test))]
mod macos;
pub mod os;
#[cfg(any(windows, target_os = "macos"))]
mod owner;
#[cfg(target_os = "linux")]
mod portal;
pub mod share;
pub mod shortcuts;
pub mod taskbar;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod user_dirs;
#[cfg(any(test, target_arch = "wasm32"))]
mod web;
#[cfg(windows)]
mod win32;

use flui_platform::capability::{CapabilityRegistrar, Plugin};

/// The system services' providers for the platform the application runs on:
/// the Windows shell on Windows, AppKit and the system frameworks on macOS,
/// the XDG desktop portal on Linux, the browser on the web, and a refusal
/// for what a platform's provider does not have.
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
        #[cfg(target_os = "macos")]
        macos::register(registrar);
        #[cfg(target_os = "linux")]
        portal::register(registrar, None, None);
        #[cfg(target_arch = "wasm32")]
        web::register(registrar);
        #[cfg(not(any(windows, target_arch = "wasm32")))]
        refusals::register(registrar);
    }
}

/// The refusals off Windows: the services no portal has, the jump list, and
/// what a platform's provider does not have yet.
#[cfg(not(any(windows, target_arch = "wasm32")))]
mod refusals {
    use flui_platform::capability::{Capability, CapabilityRegistrar, ProviderContext};
    use flui_platform::{Unsupported, UnsupportedReason};

    /// Provide `C` as a refusal for `reason`.
    fn refuse<C: Capability>(registrar: &mut CapabilityRegistrar, reason: UnsupportedReason) {
        registrar.provide::<C>(move |_: &ProviderContext<'_>| {
            Err::<C::Handle, _>(Unsupported::of::<C>(reason))
        });
    }

    /// macOS provides every service but the jump list, which is Windows'.
    #[cfg(target_os = "macos")]
    pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
        refuse::<crate::jump_list::JumpListCapability>(
            registrar,
            UnsupportedReason::NotOnThisPlatform,
        );
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
        let not_yet = UnsupportedReason::NotYetOnThisHost;
        // The portal's registration provides the rest on Linux, and refuses
        // the message box, the share sheet and the jump list for good.
        #[cfg(not(target_os = "linux"))]
        {
            use crate::autostart::AutostartCapability;
            use crate::dialog::{FileDialogCapability, MessageBoxCapability};
            use crate::jump_list::JumpListCapability;
            use crate::share::ShareCapability;
            use crate::shortcuts::GlobalShortcutCapability;
            use crate::taskbar::TaskbarCapability;
            use crate::user_dirs::UserDirCapability;
            use flui_platform::launch::LauncherCapability;

            refuse::<LauncherCapability>(registrar, not_yet);
            refuse::<FileDialogCapability>(registrar, not_yet);
            refuse::<MessageBoxCapability>(registrar, not_yet);
            refuse::<ShareCapability>(registrar, not_yet);
            refuse::<AutostartCapability>(registrar, not_yet);
            refuse::<GlobalShortcutCapability>(registrar, not_yet);
            refuse::<TaskbarCapability>(registrar, not_yet);
            refuse::<UserDirCapability>(registrar, not_yet);
            refuse::<JumpListCapability>(registrar, UnsupportedReason::NotOnThisPlatform);
        }
        refuse::<crate::os::OsCapability>(registrar, not_yet);
    }
}
