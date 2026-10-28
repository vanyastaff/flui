//! The application's lifecycle as each host hears it: the user's session
//! ending, the machine sleeping and waking, the session locking and
//! unlocking, memory pressure, a reopen and an activation.
//!
//! A host's source hears the operating system and hands a notice to the
//! loop's notice channel ([`notice`], behind the owner's `NoticeChannel`) and
//! a session end's phases to the loop's application delivery, which asks the
//! application's handler; the host's table makes the source and the channel
//! for the loop. Each loop is its table's lifecycle position. A loop that
//! hears the lifecycle in its own callbacks (AppKit's and UIKit's application
//! delegates, the Activity's, the winit loop's turns on Linux) answers the
//! release of a session end itself (`LifecycleHooks`), and one position serves
//! all of them here; the Windows and web loops answer their own, and the
//! headless host's position is its test handle.
//!
//! The platform files: `windows` (the session, power, lock and memory
//! broadcasts the owner's receiver hears), `apple` (the AppKit and dispatch
//! sources of macOS; UIKit's arrive at its application delegate), `android`
//! (what the Activity's callbacks mean), `web` (`pagehide` and `pageshow`),
//! `linux` (logind and the kernel's memory pressure, which every Linux host
//! reads) and `headless` (what a test reports).

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod apple;
pub(crate) mod headless;
#[cfg(any(
    all(target_os = "linux", any(feature = "winit-backend", feature = "x11")),
    test
))]
pub(crate) mod linux;
pub(crate) mod notice;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;

/// The lifecycle position of every loop that hears its host's lifecycle in
/// its own callbacks: macOS, iOS, Android, and the winit host on Linux.
#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    all(target_os = "linux", any(feature = "winit-backend", feature = "x11"))
))]
impl<L: crate::owner::notices::LifecycleHooks> crate::host::Lifecycle<L> for L {
    fn release_session_end(owner: &L) {
        owner.release_session_end();
    }

    fn activate(_: &L, _: bool) {
        // None of these hosts brings the application forward on request:
        // AppKit's is activated once, as its loop starts; UIKit and Android
        // decide which application is in front; winit raises one window at a
        // time (`Window::focus_window`).
    }
}
