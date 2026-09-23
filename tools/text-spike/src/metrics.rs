//! Peak-RSS measurement via `getrusage`.
//!
//! `ru_maxrss` units differ by platform: bytes on macOS, kilobytes on
//! Linux. Both arms are provided since this spike may run on either host;
//! whichever ran it, the report must state which arm produced the number.

#[cfg(target_os = "macos")]
pub fn peak_rss_bytes() -> u64 {
    // SAFETY: `usage` is zero-initialized and `getrusage` only writes
    // through the pointer we pass it; RUSAGE_SELF is always valid.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &raw mut usage);
        usage.ru_maxrss as u64
    }
}

#[cfg(target_os = "linux")]
pub fn peak_rss_bytes() -> u64 {
    // SAFETY: `usage` is zero-initialized and `getrusage` only writes
    // through the pointer we pass it; RUSAGE_SELF is always valid.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &raw mut usage);
        // Linux reports ru_maxrss in KB, not bytes.
        (usage.ru_maxrss as u64) * 1024
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn peak_rss_bytes() -> u64 {
    0
}
