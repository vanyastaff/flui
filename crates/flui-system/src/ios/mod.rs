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
//! UIKit or the system calls goes through `owner::panic_boundary::os_entry`,
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
pub(crate) use uikit::providers;

/// The iOS providers.
#[cfg(target_os = "ios")]
mod uikit {
    use flui_platform::capability::ProviderContext;
    use flui_platform::launch::{Launcher, LauncherCapability};
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2_ui_kit::UIDevice;

    use crate::apple::bookmark::AppleGrants;
    use crate::apple::main_thread;
    use crate::apple::os::AppleOs;
    use crate::apple::user_dirs::AppleUserDirs;
    use crate::apple::version::{OnIOS, RunningVersion};
    use crate::dialog::{FileDialogCapability, FileDialogs, MessageBoxCapability, MessageBoxes};
    use crate::grant::Grants;
    use crate::off_owner::OffOwner;
    use crate::os::{Os, OsCapability};
    use crate::providers::{Providers, Provision};
    use crate::share::{Share, ShareCapability};
    use crate::user_dirs::UserDirs;

    /// The iOS providers of the services iOS has, and the refusals of the
    /// others.
    pub(crate) fn providers() -> Providers {
        Providers {
            launcher: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<LauncherCapability>()?;
                Ok::<_, Unsupported>(Launcher::new(super::launcher::IOSLauncher::new(mtm), ctx))
            }),
            file_dialogs: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<FileDialogCapability>()?;
                FileDialogs::try_new(
                    super::dialogs::IOSDialogs::new(
                        mtm,
                        ctx.runtime_local::<RunningVersion<OnIOS>>().0,
                        OffOwner::of(ctx),
                        ctx.completions().clone(),
                    ),
                    ctx,
                )
            }),
            message_boxes: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<MessageBoxCapability>()?;
                MessageBoxes::try_new(super::message_box::IOSMessageBoxes::new(mtm), ctx)
            }),
            share: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<ShareCapability>()?;
                Share::try_new(
                    super::share::IOSShare::new(mtm, OffOwner::of(ctx), ctx.completions().clone()),
                    ctx,
                )
            }),
            user_dirs: Provision::provided(|ctx: &ProviderContext<'_>| {
                Ok::<_, Unsupported>(UserDirs::new(AppleUserDirs::new(OffOwner::of(ctx)), ctx))
            }),
            grants: Provision::provided(|ctx: &ProviderContext<'_>| {
                Ok::<_, Unsupported>(Grants::new(
                    AppleGrants::new(
                        OffOwner::of(ctx),
                        crate::sandbox::ios_app(&|name| std::env::var_os(name)),
                    ),
                    ctx,
                ))
            }),
            os: Provision::provided(|ctx: &ProviderContext<'_>| {
                // The name the device says: iOS on an iPhone, iPadOS on an iPad.
                let name = UIDevice::currentDevice(main_thread::<OsCapability>()?).systemName();
                let version = ctx.runtime_local::<RunningVersion<OnIOS>>().0;
                Ok::<_, Unsupported>(Os::new(
                    AppleOs::new(name.to_string(), version.components()),
                    ctx,
                ))
            }),
            // iOS has no form of autostart, of a shortcut heard while another
            // application is in front or of a jump list.
            autostart: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            global_shortcuts: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            jump_list: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
            // Its form of the launcher entry, the count on the application's
            // icon, needs the user's permission for notifications, which no
            // provider asks for yet.
            taskbar: Provision::Refused(UnsupportedReason::NotYetOnThisHost),
        }
    }
}
