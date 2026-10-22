//! The Windows displays: the monitors the system reports ([`source`]), what a
//! sample of them holds and answers with no Win32 call ([`sample`]), and the
//! system's display broadcasts, which mark the sample stale.

#[cfg(all(windows, test))]
mod message_tests;
pub(crate) mod sample;
#[cfg(windows)]
pub(crate) mod source;

#[cfg(windows)]
pub(crate) use source::WindowsDisplays;
