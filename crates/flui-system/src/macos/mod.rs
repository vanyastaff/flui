//! The macOS providers: AppKit, Foundation, Launch Services,
//! ServiceManagement and Carbon's hot keys, through objc2.
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
//! - **On a Grand Central Dispatch queue** ([`queue`]): every request that
//!   touches the file system or waits on a daemon (the launcher, the share
//!   picker's file check, the login item, the user's folders).
//!
//! Every block, work item, method and Carbon handler AppKit or the system
//! calls goes through `owner::panic_boundary::os_entry`. A feature newer
//! than the floor is behind a gate of [`version`]. The rules of each
//! provider (what a request becomes, what an answer means) are tested
//! beside it on every host; the AppKit calls compile on macOS only.

// Each provider's AppKit part is a `macos`-only module of its file, which
// expects the workspace's `unsafe_code` lint where it calls the system.
mod autostart;
mod dialogs;
mod floor;
mod launcher;
mod message_box;
mod os;
mod queue;
mod share;
mod sheet;
mod shortcuts;
mod taskbar;
mod user_dirs;
mod version;

use flui_platform::SystemFailure;

/// An `NSError`'s code, by what it is a code of: what [`failure`] and each
/// provider's own mapping read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorCode {
    /// A POSIX `errno`: the error's own, in `NSPOSIXErrorDomain`, or the one
    /// a Cocoa error carries as its underlying error.
    Posix(i32),
    /// A code of `NSCocoaErrorDomain` with no POSIX error under it.
    Cocoa(isize),
    /// A code of any other domain (ServiceManagement's, `NSOSStatus`).
    Other(isize),
}

/// The [`SystemFailure`] an error no variant of the service's names is.
pub(crate) fn failure(code: ErrorCode) -> SystemFailure {
    match code {
        ErrorCode::Posix(errno) => SystemFailure::Os(errno),
        ErrorCode::Cocoa(code) | ErrorCode::Other(code) => {
            SystemFailure::Framework(i64::try_from(code).unwrap_or(i64::MAX))
        }
    }
}

/// The `errno` values the providers name, Darwin's.
pub(crate) mod errno {
    /// Operation not permitted.
    pub(crate) const EPERM: i32 = 1;
    /// No such file or directory.
    pub(crate) const ENOENT: i32 = 2;
    /// Permission denied.
    pub(crate) const EACCES: i32 = 13;
    /// A component of the path is not a directory.
    pub(crate) const ENOTDIR: i32 = 20;
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::register;

/// What the providers share on macOS: the window a widget is in, the main
/// thread, the version, an `NSError`'s code, and the registration.
#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "a raw window handle's view, Foundation's error constants"
)]
mod appkit {
    use std::path::Path;

    use flui_platform::capability::{Capability, CapabilityRegistrar, ProviderContext, WindowRef};
    use flui_platform::launch::{Launcher, LauncherCapability};
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindow};
    use objc2_foundation::{NSDictionary, NSError, NSString, NSURL};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::version::MacOSVersion;
    use super::{ErrorCode, queue::WorkQueue};
    use crate::autostart::{Autostart, AutostartCapability};
    use crate::dialog::{FileDialogCapability, FileDialogs, MessageBoxCapability, MessageBoxes};
    use crate::os::{Os, OsCapability};
    use crate::share::{Share, ShareCapability};
    use crate::shortcuts::{GlobalShortcutCapability, GlobalShortcuts};
    use crate::taskbar::{TaskbarCapability, TaskbarEntry};
    use crate::user_dirs::{UserDirCapability, UserDirs};

    /// The macOS version, read when a UI runtime first asks for it.
    pub(crate) struct RunningVersion(pub(crate) MacOSVersion);

    impl Default for RunningVersion {
        fn default() -> Self {
            Self(MacOSVersion::read())
        }
    }

    /// The main thread, for a service AppKit runs there; `C` refused with
    /// `NotYetOnThisHost` on a runtime whose owner thread is another one.
    fn main_thread<C: Capability>() -> Result<MainThreadMarker, Unsupported> {
        MainThreadMarker::new()
            .ok_or_else(|| Unsupported::of::<C>(UnsupportedReason::NotYetOnThisHost))
    }

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

    /// `path` as a file URL, from its bytes as the file system has them, so
    /// a name that is not UTF-8 (a FAT, exFAT, SMB or NFS volume may hold
    /// one) names the same file; `None` for a path holding a NUL, which no
    /// file has. `directory` says whether it names a folder.
    pub(crate) fn file_url(path: &Path, directory: bool) -> Option<Retained<NSURL>> {
        use std::os::unix::ffi::OsStrExt as _;

        let bytes = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let pointer = std::ptr::NonNull::new(bytes.as_ptr().cast_mut())?;
        // SAFETY: a NUL-terminated byte string that outlives the call, which
        // copies it into the URL.
        Some(unsafe {
            NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                pointer, directory, None,
            )
        })
    }

    /// The path a file URL names, as the file system's bytes; `None` for a
    /// URL that names no file. `getFileSystemRepresentation:` answers `NO`
    /// where `fileSystemRepresentation` would raise an exception.
    pub(crate) fn url_path(url: &NSURL) -> Option<std::path::PathBuf> {
        use std::os::unix::ffi::OsStrExt as _;

        if !url.isFileURL() {
            return None;
        }
        let mut buffer = vec![0_u8; usize::try_from(libc::PATH_MAX).unwrap_or(1024) + 1];
        let pointer = std::ptr::NonNull::new(buffer.as_mut_ptr().cast::<std::ffi::c_char>())?;
        // SAFETY: the buffer outlives the call and holds the length given,
        // which the method writes at most, a NUL included.
        let written = unsafe { url.getFileSystemRepresentation_maxLength(pointer, buffer.len()) };
        if !written {
            return None;
        }
        let length = buffer.iter().position(|byte| *byte == 0)?;
        buffer.truncate(length);
        Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
            &buffer,
        )))
    }

    /// What `error`'s code is a code of: its POSIX code where it carries
    /// one, else its own code by its domain.
    pub(crate) fn error_code(error: &NSError) -> ErrorCode {
        // SAFETY: Foundation's error domain and key constants are immutable
        // strings that live for the process.
        let (posix, cocoa, underlying_key) = unsafe {
            (
                objc2_foundation::NSPOSIXErrorDomain,
                objc2_foundation::NSCocoaErrorDomain,
                objc2_foundation::NSUnderlyingErrorKey,
            )
        };
        let code = error.code();
        let domain = error.domain();
        if &*domain == posix {
            return ErrorCode::Posix(i32::try_from(code).unwrap_or(i32::MAX));
        }
        let info: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> = error.userInfo();
        let underlying = info
            .objectForKey(underlying_key)
            .and_then(|object| object.downcast::<NSError>().ok());
        match underlying {
            Some(underlying) if &*underlying.domain() == posix => {
                ErrorCode::Posix(i32::try_from(underlying.code()).unwrap_or(i32::MAX))
            }
            _ if &*domain == cocoa => ErrorCode::Cocoa(code),
            _ => ErrorCode::Other(code),
        }
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
                super::dialogs::MacDialogs::new(mtm, ctx.runtime_local::<RunningVersion>().0),
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
                    ctx.runtime_local::<RunningVersion>().0,
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
                super::user_dirs::MacUserDirs::new(ctx.runtime_local::<WorkQueue>()),
                ctx,
            ))
        });
        registrar.provide::<OsCapability>(|ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Os::new(
                super::os::MacOs::new(ctx.runtime_local::<RunningVersion>().0),
                ctx,
            ))
        });
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::SystemFailure;

    use super::{ErrorCode, failure};

    #[test]
    fn an_errors_posix_code_is_the_oss_and_any_other_the_frameworks() {
        assert_eq!(failure(ErrorCode::Posix(28)), SystemFailure::Os(28));
        assert_eq!(
            failure(ErrorCode::Cocoa(3328)),
            SystemFailure::Framework(3328)
        );
        assert_eq!(
            failure(ErrorCode::Other(-9878)),
            SystemFailure::Framework(-9878)
        );
    }
}
