//! The system's preferences (ADR-0172): text, motion, contrast, gesture and
//! wheel settings and the preferred languages, as each host reads them, and how
//! a host wakes its owner when one of them changes.
//!
//! The model is what two or more hosts share:
//!
//! - `read`: the admission of a bounded native read, which the Android and
//!   AppKit sources take before they ask the system again;
//! - `locales`: the preferred languages from the forms the systems spell them
//!   in;
//! - `regional`: the regional formats (ADR-0278) from the values the hosts
//!   report them as.
//!
//! Each platform file is its host's source: `windows` (WinRT and Win32
//! settings, told of changes by the owner's broadcast receiver), `apple`
//! (AppKit and UIKit), `android` (the Activity's configuration and settings),
//! `web` (media queries and `navigator.languages`), `linux` (the Settings
//! portal on D-Bus, read by every Linux host) and `headless` (the values a
//! test sets). A loop keeps its source through the owner's hook
//! (`owner::sources::SystemPreferenceSource` or the loop's own), and the host's
//! table builds the source and names its type as the preference position.

#[cfg(any(
    target_arch = "wasm32",
    linux_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
pub(crate) mod locales;
#[cfg(any(target_os = "android", target_os = "macos", test))]
pub(crate) mod read;
#[cfg(any(
    target_arch = "wasm32",
    windows,
    linux_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
pub(crate) mod regional;

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
pub(crate) mod headless;
#[cfg(linux_host)]
pub(crate) mod linux;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;
