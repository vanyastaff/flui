//! The host's own font fallback answer (ADR-0271 §3): which faces the
//! operating system draws each script and emoji with, for a locale, as the
//! [`SystemFallback`] the text stack merges ahead of FLUI's tables.
//!
//! The composition root hands [`system_fallback`] to the font feed, which
//! calls it off the owner thread. Each platform file asks its system once
//! per script; one that cannot answer gives [`SystemFallback::none`], so
//! FLUI's tables alone, never a panic.

use flui_painting::SystemFallback;

#[cfg(any(target_os = "android", test))]
mod android;
#[cfg(all(
    unix,
    not(any(target_os = "android", target_os = "macos", target_os = "ios"))
))]
mod linux;

#[cfg(target_os = "android")]
use android::system_fallback as host_answer;
#[cfg(all(
    unix,
    not(any(target_os = "android", target_os = "macos", target_os = "ios"))
))]
use linux::system_fallback as host_answer;

/// No answer where FLUI does not ask the host yet.
#[cfg(not(all(unix, not(any(target_os = "macos", target_os = "ios")))))]
fn host_answer(locale: &str) -> SystemFallback {
    SystemFallback::none(locale)
}

/// The faces the operating system falls back on for `locale`, a BCP 47
/// tag such as `ja-JP`: fontconfig's order on Linux and the BSDs, the
/// system's `fonts.xml` on Android. Windows, Apple and the web give no
/// answer yet (`docs/gaps.md`, entry 35).
///
/// Blocking: it reads the system's configuration and may scan its fonts,
/// so it belongs on the font feed's thread, not the owner's.
#[must_use]
pub fn system_fallback(locale: &str) -> SystemFallback {
    host_answer(locale)
}
