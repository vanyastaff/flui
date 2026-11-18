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
//! calls goes through `owner::panic_boundary::os_entry`. A feature newer
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
pub(crate) use appkit::register;

/// What the providers share on macOS alone: the window a widget is in, and
/// the registration.
#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "a raw window handle's view")]
mod appkit {
    use flui_platform::Unsupported;
    use flui_platform::capability::{CapabilityRegistrar, ProviderContext, WindowRef};
    use flui_platform::launch::{Launcher, LauncherCapability};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindow};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use crate::apple::bookmark::AppleGrants;
    use crate::apple::main_thread;
    use crate::apple::os::AppleOs;
    use crate::apple::queue::WorkQueue;
    use crate::apple::user_dirs::AppleUserDirs;
    use crate::apple::version::{OnMacOS, RunningVersion};
    use crate::autostart::{Autostart, AutostartCapability};
    use crate::dialog::{FileDialogCapability, FileDialogs, MessageBoxCapability, MessageBoxes};
    use crate::grant::{GrantCapability, Grants};
    use crate::os::{Os, OsCapability};
    use crate::share::{Share, ShareCapability};
    use crate::shortcuts::{GlobalShortcutCapability, GlobalShortcuts};
    use crate::taskbar::{TaskbarCapability, TaskbarEntry};
    use crate::user_dirs::{UserDirCapability, UserDirs};

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

    /// Register the macOS providers of every service but the jump list,
    /// which is Windows'.
    pub(crate) fn register(registrar: &mut CapabilityRegistrar) {
        registrar.provide::<LauncherCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Launcher::new(
                super::launcher::MacLauncher::new(ctx.runtime_local::<WorkQueue>()),
                ctx,
            ))
        });
        registrar.provide::<FileDialogCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<FileDialogCapability>()?;
            FileDialogs::try_new(
                super::dialogs::MacDialogs::new(
                    mtm,
                    ctx.runtime_local::<RunningVersion<OnMacOS>>().0,
                    ctx.runtime_local::<WorkQueue>(),
                ),
                ctx,
            )
        });
        registrar.provide::<MessageBoxCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<MessageBoxCapability>()?;
            MessageBoxes::try_new(super::message_box::MacMessageBoxes::new(mtm), ctx)
        });
        registrar.provide::<ShareCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<ShareCapability>()?;
            Share::try_new(
                super::share::MacShare::new(
                    mtm,
                    ctx.runtime_local::<WorkQueue>(),
                    ctx.completions().clone(),
                ),
                ctx,
            )
        });
        registrar.provide::<AutostartCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Autostart::new(
                super::autostart::MacAutostart::new(
                    ctx.runtime_local::<RunningVersion<OnMacOS>>().0,
                    ctx.runtime_local::<WorkQueue>(),
                ),
                ctx,
            ))
        });
        registrar.provide::<GlobalShortcutCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<GlobalShortcutCapability>()?;
            Ok::<_, Unsupported>(GlobalShortcuts::new(
                super::shortcuts::MacShortcuts::new(
                    mtm,
                    ctx.runtime_local::<super::shortcuts::HotKeys>(),
                ),
                ctx,
            ))
        });
        registrar.provide::<TaskbarCapability>(|ctx: &ProviderContext<'_>| {
            let mtm = main_thread::<TaskbarCapability>()?;
            Ok::<_, Unsupported>(TaskbarEntry::new(
                super::taskbar::MacDockTile::new(
                    mtm,
                    ctx.runtime_local::<super::taskbar::DockViews>(),
                ),
                ctx,
            ))
        });
        registrar.provide::<UserDirCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(UserDirs::new(
                AppleUserDirs::new(ctx.runtime_local::<WorkQueue>()),
                ctx,
            ))
        });
        registrar.provide::<OsCapability>(|ctx: &ProviderContext<'_>| {
            let version = ctx.runtime_local::<RunningVersion<OnMacOS>>().0;
            Ok::<_, Unsupported>(Os::new(AppleOs::new("macOS", version.components()), ctx))
        });
        registrar.provide::<GrantCapability>(|ctx: &ProviderContext<'_>| {
            // No Flatpak description on macOS: the App Sandbox is read from
            // the environment its system sets, with no file system call.
            let confinement = flui_platform::sandbox::Confinement::of_process(
                &flui_platform::sandbox::FlatpakInfo::Absent,
                &|_| None,
            );
            Ok::<_, Unsupported>(Grants::new(
                AppleGrants::new(
                    ctx.runtime_local::<WorkQueue>(),
                    crate::sandbox::of_confinement(&confinement),
                ),
                ctx,
            ))
        });
    }
}
