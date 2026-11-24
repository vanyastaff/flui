//! FLUI's OS shell services, the first capability crate (ADR-0154 §11).
//!
//! - **Launching**: open a URI, a file or a folder in the system's handler,
//!   show a file in the file manager, move it to the trash. The handle,
//!   [`flui_platform::launch::Launcher`], lives in the contract crate, since
//!   framework widgets name it (ADR-0154 §3); this crate provides it.
//! - **Dialogs**: [`dialog::FileDialogs`] and [`dialog::MessageBoxes`],
//!   modal to the acquiring widget's window, one dialog per window.
//! - **Grants**: [`grant::Grants`], a picked file reacquired after a restart
//!   through the access its pick carried, never as a path the process's
//!   [`sandbox::Sandbox`] keeps it from opening.
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
//! A platform provides the services it has a form of and refuses the others
//! with an [`Unsupported`](flui_platform::Unsupported) whose reason says
//! whether it never will (`NotOnThisPlatform`) or does not yet
//! (`NotYetOnThisHost`), never with a stand-in (ADR-0154 §10). Which
//! platform provides which service, and where each falls short of the
//! contract, is stated once: in `docs/gaps.md` ("The OS shell services") and
//! the System services rows of `crates/flui-native/platform-parity.toml`.

#[cfg(any(target_os = "android", test))]
mod android;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
mod apple;
pub mod autostart;
pub mod dialog;
pub mod grant;
#[cfg(any(target_os = "ios", test))]
mod ios;
pub mod jump_list;
#[cfg(any(target_os = "macos", test))]
mod macos;
mod off_owner;
pub mod os;
#[cfg(any(
    windows,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
mod owner;
#[cfg(target_os = "linux")]
mod portal;
mod providers;
pub mod sandbox;
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
/// UIKit and Foundation on iOS, the XDG desktop portal on Linux, the
/// Activity's `Intent`s and document pickers on Android, the browser on the
/// web, and a refusal for what a platform's provider does not have.
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
        platform_providers().register(registrar);
    }
}

/// This platform's answer for every service.
#[cfg(windows)]
use win32::providers as platform_providers;

/// This platform's answer for every service.
#[cfg(target_os = "macos")]
use macos::providers as platform_providers;

/// This platform's answer for every service.
#[cfg(target_os = "ios")]
use ios::providers as platform_providers;

/// This platform's answer for every service.
#[cfg(target_os = "android")]
use android::providers as platform_providers;

/// This platform's answer for every service.
#[cfg(target_arch = "wasm32")]
use web::providers as platform_providers;

/// This platform's answer for every service: the portal on the session bus,
/// naming the application as the composition root does.
#[cfg(target_os = "linux")]
fn platform_providers() -> providers::Providers {
    portal::providers(None, None)
}

/// A platform FLUI has no provider on yet refuses every service with
/// `NotYetOnThisHost`, and the jump list, which is Windows', for good.
#[cfg(not(any(
    windows,
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_arch = "wasm32"
)))]
fn platform_providers() -> providers::Providers {
    use flui_platform::UnsupportedReason::{NotOnThisPlatform, NotYetOnThisHost};
    use providers::Provision::Refused;

    providers::Providers {
        launcher: Refused(NotYetOnThisHost),
        file_dialogs: Refused(NotYetOnThisHost),
        message_boxes: Refused(NotYetOnThisHost),
        share: Refused(NotYetOnThisHost),
        autostart: Refused(NotYetOnThisHost),
        global_shortcuts: Refused(NotYetOnThisHost),
        taskbar: Refused(NotYetOnThisHost),
        user_dirs: Refused(NotYetOnThisHost),
        jump_list: Refused(NotOnThisPlatform),
        os: Refused(NotYetOnThisHost),
        grants: Refused(NotYetOnThisHost),
    }
}
