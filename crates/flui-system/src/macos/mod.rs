//! The macOS providers: AppKit, Foundation, Launch Services,
//! ServiceManagement and Carbon's hot keys, through objc2. What they share
//! with iOS (Foundation's side of a provider) is `crate::apple`.
//!
//! Each service runs where its framework puts it:
//!
//! - **On the main thread**, which is the UI runtime's owner thread in a
//!   macOS application: the open and save panels and the alert, each a
//!   sheet on the acquiring widget's window; the share picker, a popover on
//!   it; the Dock tile; the hot keys, whose events Carbon dispatches on the
//!   main run loop. A runtime whose owner thread is not the main thread
//!   (the headless host) has no path to AppKit, and those services are
//!   refused there with `NotYetOnThisHost`, before AppKit is touched.
//! - **On a Grand Central Dispatch queue** (`apple::queue`): every request
//!   that touches the file system or waits on a daemon (the launcher, the
//!   share picker's file check, the login item, the user's folders).
//!
//! Every block, work item, method and Carbon handler AppKit or the system
//! calls goes through `flui_foundation::panic::os_entry`. A feature newer
//! than the floor is behind a gate of `apple::version`. The rules of each
//! provider (what a request becomes, what an answer means) are tested
//! beside it on every host; the AppKit calls compile on macOS only.

// Each provider's AppKit part is a `macos`-only module of its file, which
// expects the workspace's `unsafe_code` lint where it calls the system.
mod autostart;
mod dialogs;
mod launcher;
mod message_box;
mod share;
mod shortcuts;
mod taskbar;

#[cfg(target_os = "macos")]
pub(crate) use appkit::providers;

/// What the providers share on macOS alone: the window a widget is in, and
/// the providers.
#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "a raw window handle's view")]
mod appkit {
    use flui_platform::capability::{ProviderContext, WindowRef};
    use flui_platform::launch::Launcher;
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindow};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use crate::apple::bookmark::AppleGrants;
    use crate::apple::main_thread;
    use crate::apple::os::AppleOs;
    use crate::apple::user_dirs::AppleUserDirs;
    use crate::apple::version::{OnMacOS, RunningVersion};
    use crate::autostart::Autostart;
    use crate::dialog::{FileDialogCapability, FileDialogs, MessageBoxCapability, MessageBoxes};
    use crate::grant::Grants;
    use crate::off_owner::OffOwner;
    use crate::os::Os;
    use crate::providers::{Providers, Provision};
    use crate::share::{Share, ShareCapability};
    use crate::shortcuts::{GlobalShortcutCapability, GlobalShortcuts};
    use crate::taskbar::{TaskbarCapability, TaskbarEntry};
    use crate::user_dirs::UserDirs;

    /// The AppKit window `window` is, when its handle names a view in one:
    /// `None` for a window of another kind (a headless window) or a view
    /// out of any window.
    pub(crate) fn ns_window(
        window: &WindowRef,
        _mtm: MainThreadMarker,
    ) -> Option<Retained<NSWindow>> {
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: an AppKit handle names a live `NSView` of this process, and
        // the handle is read on the main thread, where the view is used.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    /// The macOS providers: every service but the jump list, which is refused.
    pub(crate) fn providers() -> Providers {
        Providers {
            launcher: Provision::provided(|ctx: &ProviderContext<'_>| {
                Ok::<_, Unsupported>(Launcher::new(
                    super::launcher::MacLauncher::new(OffOwner::of(ctx)),
                    ctx,
                ))
            }),
            file_dialogs: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<FileDialogCapability>()?;
                FileDialogs::try_new(
                    super::dialogs::MacDialogs::new(
                        mtm,
                        ctx.runtime_local::<RunningVersion<OnMacOS>>().0,
                        OffOwner::of(ctx),
                    ),
                    ctx,
                )
            }),
            message_boxes: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<MessageBoxCapability>()?;
                MessageBoxes::try_new(super::message_box::MacMessageBoxes::new(mtm), ctx)
            }),
            share: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<ShareCapability>()?;
                Share::try_new(
                    super::share::MacShare::new(mtm, OffOwner::of(ctx), ctx.completions().clone()),
                    ctx,
                )
            }),
            autostart: Provision::provided(|ctx: &ProviderContext<'_>| {
                Ok::<_, Unsupported>(Autostart::new(
                    super::autostart::MacAutostart::new(
                        ctx.runtime_local::<RunningVersion<OnMacOS>>().0,
                        OffOwner::of(ctx),
                    ),
                    ctx,
                ))
            }),
            global_shortcuts: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<GlobalShortcutCapability>()?;
                Ok::<_, Unsupported>(GlobalShortcuts::new(
                    super::shortcuts::MacShortcuts::new(
                        mtm,
                        ctx.runtime_local::<super::shortcuts::HotKeys>(),
                    ),
                    ctx,
                ))
            }),
            taskbar: Provision::provided(|ctx: &ProviderContext<'_>| {
                let mtm = main_thread::<TaskbarCapability>()?;
                Ok::<_, Unsupported>(TaskbarEntry::new(
                    super::taskbar::MacDockTile::new(
                        mtm,
                        ctx.runtime_local::<super::taskbar::DockViews>(),
                    ),
                    ctx,
                ))
            }),
            user_dirs: Provision::provided(|ctx: &ProviderContext<'_>| {
                Ok::<_, Unsupported>(UserDirs::new(AppleUserDirs::new(OffOwner::of(ctx)), ctx))
            }),
            os: Provision::provided(|ctx: &ProviderContext<'_>| {
                let version = ctx.runtime_local::<RunningVersion<OnMacOS>>().0;
                Ok::<_, Unsupported>(Os::new(AppleOs::new("macOS", version.components()), ctx))
            }),
            grants: Provision::provided(|ctx: &ProviderContext<'_>| {
                // No Flatpak description on macOS: the App Sandbox is read from
                // the environment its system sets. The user database is asked
                // only for an App Sandbox whose `HOME` is not its container.
                let confinement = flui_platform::sandbox::Confinement::of_process(
                    &flui_platform::sandbox::FlatpakInfo::Absent,
                    &crate::sandbox::user_database_home,
                    &|_| None,
                );
                Ok::<_, Unsupported>(Grants::new(
                    AppleGrants::new(
                        OffOwner::of(ctx),
                        crate::sandbox::of_confinement(&confinement),
                    ),
                    ctx,
                ))
            }),
            // The jump list is Windows'.
            jump_list: Provision::Refused(UnsupportedReason::NotOnThisPlatform),
        }
    }
}
