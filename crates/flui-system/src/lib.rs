//! FLUI's OS shell services, the first capability crate (ADR-0154 §11).
//!
//! - **Launching**: open a URI, a file or a folder in the system's handler,
//!   show a file in the file manager, move it to the trash. The handle,
//!   [`Launcher`](flui_platform::launch::Launcher), lives in
//!   `flui_platform::launch`, since framework widgets name it (ADR-0154 §3);
//!   this crate provides it.
//! - **File dialogs**: [`dialog::FileDialogs`], open and save dialogs modal to
//!   the acquiring widget's window.
//!
//! An application registers [`SystemPlugin`]; a scenario registers
//! `testing::ScriptedSystem` instead (feature `testing`).
//!
//! No host backs these services yet: until a platform's provider lands, its
//! handles are refused with [`UnsupportedReason::NotYetOnThisHost`], never
//! faked (ADR-0154 §10).

pub mod dialog;
#[cfg(feature = "testing")]
pub mod testing;

use flui_platform::capability::{CapabilityRegistrar, Plugin, ProviderContext};
use flui_platform::launch::{Launcher, LauncherCapability};
use flui_platform::{Unsupported, UnsupportedReason};

use dialog::{FileDialogCapability, FileDialogs};

/// The system services' providers for the platform the application runs on.
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
