//! The Windows displays: the monitors the system reports ([`source`]), what a
//! sample of them holds and answers with no Win32 call ([`sample`]), and the
//! system's display broadcasts, which mark the sample stale (`broadcasts`).
//!
//! The Win32 loop holds the monitors through its `HostDisplays` hook and the
//! broadcast receiver tells [`DisplayBroadcasts`] what the system broadcast;
//! the host's table makes both. The winit host on Windows reads the same
//! monitors through its own loop's hook.

#[cfg(windows)]
mod broadcasts;
#[cfg(all(windows, test))]
mod message_tests;
pub(crate) mod sample;
#[cfg(windows)]
pub(crate) mod source;

#[cfg(windows)]
pub(crate) use broadcasts::DisplayBroadcasts;
#[cfg(windows)]
pub(crate) use source::WindowsDisplays;
