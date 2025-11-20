//! Debug utilities and flags
//!
//! This module provides debugging utilities and flags for FLUI development.

use std::sync::atomic::{AtomicBool, Ordering};

/// Debug flags for enabling/disabling various debugging features
#[derive(Debug, Clone, Copy, Default)]
pub struct DebugFlags {
    /// Enable verbose logging
    pub verbose: bool,
    /// Enable layout debugging
    pub layout: bool,
    /// Enable paint debugging
    pub paint: bool,
    /// Enable performance profiling
    pub profiling: bool,
}

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable debug output
pub fn enable_debug() {
    DEBUG_ENABLED.store(true, Ordering::Relaxed);
}

/// Disable debug output
pub fn disable_debug() {
    DEBUG_ENABLED.store(false, Ordering::Relaxed);
}

/// Check if debug output is enabled
pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Debug print macro - only prints if debug is enabled
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        if $crate::debug::is_debug_enabled() {
            println!($($arg)*);
        }
    };
}
