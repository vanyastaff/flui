//! The iOS providers: UIKit's document picker, alert and share sheet, and
//! `UIApplication`'s `openURL:`, through objc2; the user's folders and the
//! system information are Foundation's, shared with macOS (`crate::apple`).
//!
//! Each service runs where its framework puts it:
//!
//! - **On the main thread**, which is the UI runtime's owner thread in an
//!   iOS application: every UIKit call. A runtime whose owner thread is not
//!   the main thread (the headless host) has no path to UIKit, and the
//!   launcher, the dialogs, the message box, the share sheet and the system
//!   information are refused there with `NotYetOnThisHost`, before UIKit
//!   is touched.
//! - **On a Grand Central Dispatch queue** (`apple::queue`): judging and
//!   reading a picked document, writing the file a save exports, checking a
//!   shared file, the user's folders.
//!
//! A picker, an alert and a share sheet are presented from the requesting
//! window's top controller ([`presenter`]), and each ends with its window:
//! the dialogs through their hold (ADR-0344), the share sheet through a
//! watch on the window's close. Every block, delegate method and work item
//! UIKit or the system calls goes through `flui_foundation::panic::os_entry`,
//! and the one class the module declares keeps its ivars in
//! `ContainedIvars`. A feature newer than the floor is behind a gate of
//! `apple::version`.
//!
//! iOS has no form of revealing or trashing a file, of opening a document
//! in its default application, of starting at login, of a shortcut heard
//! while another application is in front or of a jump list: those answer
//! `NotOnThisPlatform`. Its form of the launcher entry is the count on the
//! application's icon, which needs the user's permission for notifications
//! and is not built: `NotYetOnThisHost`. The rules of each provider (what a
//! request becomes, what an answer means) are tested beside it on every
//! host; the UIKit calls compile on iOS only.

// Each provider's UIKit part is an `ios`-only module of its file, which
// expects the workspace's `unsafe_code` lint where it calls the system.
mod dialogs;
mod document_access;
// A page, which the tests also build for, has no file system to export to.
#[cfg(not(target_arch = "wasm32"))]
mod export;
mod launcher;
mod message_box;
mod presenter;
mod share;

#[cfg(target_os = "ios")]
pub(crate) use uikit::register;

/// The registration of the iOS providers.
#[cfg(target_os = "ios")]
mod uikit {
    use flui_platform::Unsupported;
    use flui_platform::capability::{CapabilityRegistrar, ProviderContext};
    use flui_platform::launch::{Launcher, LauncherCapability};
    use objc2_ui_kit::UIDevice;

    use crate::apple::bookmark::AppleGrants;
    use crate::apple::main_thread;
    use crate::apple::os::AppleOs;
    use crate::apple::queue::WorkQueue;
    use crate::apple::user_dirs::AppleUserDirs;
    use crate::apple::version::{OnIOS, RunningVersion};
    use crate::dialog::{FileDialogCapability, FileDialogs, MessageBoxCapability, MessageBoxes};
    use crate::grant::{GrantCapability, Grants};
    use crate::os::{Os, OsCapability};
    use crate::share::{Share, ShareCapability};
    use crate::user_dirs::{UserDirCapability, UserDirs};

    /// Register the iOS providers of the services iOS has; the refusals of
    /// the others are the crate's `refusals`.
    pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
        registrar.provide::<LauncherCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<LauncherCapability>()?;
            Ok::<_, Unsupported>(Launcher::new(super::launcher::IOSLauncher::new(mtm), ctx))
        });
        registrar.provide::<FileDialogCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<FileDialogCapability>()?;
            FileDialogs::try_new(
                super::dialogs::IOSDialogs::new(
                    mtm,
                    ctx.runtime_local::<RunningVersion<OnIOS>>().0,
                    ctx.runtime_local::<WorkQueue>(),
                    ctx.completions().clone(),
                ),
                ctx,
            )
        });
        registrar.provide::<MessageBoxCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<MessageBoxCapability>()?;
            MessageBoxes::try_new(super::message_box::IOSMessageBoxes::new(mtm), ctx)
        });
        registrar.provide::<ShareCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<ShareCapability>()?;
            Share::try_new(
                super::share::IOSShare::new(
                    mtm,
                    ctx.runtime_local::<WorkQueue>(),
                    ctx.completions().clone(),
                ),
                ctx,
            )
        });
        registrar.provide::<UserDirCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(UserDirs::new(
                AppleUserDirs::new(ctx.runtime_local::<WorkQueue>()),
                ctx,
            ))
        });
        registrar.provide::<GrantCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Grants::new(
                AppleGrants::new(
                    ctx.runtime_local::<WorkQueue>(),
                    crate::sandbox::ios_app(&|name| std::env::var_os(name)),
                ),
                ctx,
            ))
        });
        registrar.provide::<OsCapability>(|ctx: &ProviderContext<'_>| {
            // The name the device says: iOS on an iPhone, iPadOS on an iPad.
            let name = UIDevice::currentDevice(main_thread::<OsCapability>()?).systemName();
            let version = ctx.runtime_local::<RunningVersion<OnIOS>>().0;
            Ok::<_, Unsupported>(Os::new(
                AppleOs::new(name.to_string(), version.components()),
                ctx,
            ))
        });
    }
}
