//! The OS versions FLUI runs on (ADR-0244): the floors this crate compiles in, generated from
//! `platform-support.toml` ([`floor`]), and the version each environment reports with the
//! gates of what came after its floor (`gate`, built for Windows, Android, iOS and tests).
//!
//! Every feature and the owner thread read it; it names none of them.

pub(crate) mod floor;
#[cfg(any(windows, target_os = "android", target_os = "ios", test))]
pub(crate) mod gate;
