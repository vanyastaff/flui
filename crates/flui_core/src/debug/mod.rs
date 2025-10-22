//! Debug infrastructure for Flui
//!
//! This module provides debug flags, diagnostic tools, and validation infrastructure
//! for development and debugging.
//!
//! # Debug Flags
//!
//! Global debug flags control logging and validation using efficient bit flags:
//!
//! ```rust,ignore
//! use flui_core::debug::DebugFlags;
//!
//! // Enable specific debug logging
//! DebugFlags::enable(DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_MARK_NEEDS_BUILD);
//!
//! // Check if flag is enabled
//! if DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE) {
//!     println!("Building...");
//! }
//!
//! // Enable all flags
//! DebugFlags::enable_all();
//!
//! // Disable specific flags
//! DebugFlags::disable(DebugFlags::PRINT_BUILD_SCOPE);
//! ```
//!
//! # Submodules
//!
//! - `diagnostics` - Element tree diagnostic printing
//! - `lifecycle` - Lifecycle validation
//! - `key_registry` - Global key uniqueness validation

use bitflags::bitflags;
use std::sync::RwLock;

pub mod diagnostics;
pub mod key_registry;
pub mod lifecycle;

bitflags! {
    /// Global debug flags for controlling debug output
    ///
    /// These flags use efficient bit operations for fast checks at runtime.
    /// In release builds with `#[cfg(debug_assertions)]` guards, flag checks
    /// should be optimized away by the compiler.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::debug::DebugFlags;
    ///
    /// // Enable multiple flags at once
    /// DebugFlags::enable(
    ///     DebugFlags::PRINT_BUILD_SCOPE |
    ///     DebugFlags::PRINT_MARK_NEEDS_BUILD
    /// );
    ///
    /// // Check if enabled
    /// if DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE) {
    ///     println!("Building widget...");
    /// }
    ///
    /// // Enable all debugging
    /// DebugFlags::enable_all();
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DebugFlags: u32 {
        /// Print when build() is called on widgets
        const PRINT_BUILD_SCOPE = 1 << 0;

        /// Print when mark_needs_build() is called
        const PRINT_MARK_NEEDS_BUILD = 1 << 1;

        /// Print when layout() is called on RenderObjects
        const PRINT_LAYOUT = 1 << 2;

        /// Print when rebuild is scheduled
        const PRINT_SCHEDULE_BUILD = 1 << 3;

        /// Print global key registration/deregistration
        const PRINT_GLOBAL_KEY_REGISTRY = 1 << 4;

        /// Enable element lifecycle validation
        const CHECK_ELEMENT_LIFECYCLE = 1 << 5;

        /// Enable intrinsic size validation
        const CHECK_INTRINSIC_SIZES = 1 << 6;

        /// Print when InheritedWidget notifies dependents
        const PRINT_INHERITED_WIDGET_NOTIFY = 1 << 7;

        /// Print when dependencies are registered
        const PRINT_DEPENDENCIES = 1 << 8;
    }
}

impl DebugFlags {
    /// Get global debug flags instance
    pub fn global() -> &'static RwLock<Self> {
        static INSTANCE: RwLock<DebugFlags> = RwLock::new(DebugFlags::empty());
        &INSTANCE
    }

    /// Check if any of the given flags are enabled
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE) {
    ///     println!("Build scope debugging enabled");
    /// }
    /// ```
    pub fn is_enabled(flags: DebugFlags) -> bool {
        Self::global().read().unwrap().contains(flags)
    }

    /// Enable the given debug flags
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Enable multiple flags
    /// DebugFlags::enable(DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_LAYOUT);
    /// ```
    pub fn enable(flags: DebugFlags) {
        Self::global().write().unwrap().insert(flags);
    }

    /// Disable the given debug flags
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DebugFlags::disable(DebugFlags::PRINT_BUILD_SCOPE);
    /// ```
    pub fn disable(flags: DebugFlags) {
        Self::global().write().unwrap().remove(flags);
    }

    /// Enable all debug flags
    ///
    /// Useful for maximum debugging verbosity.
    pub fn enable_all() {
        *Self::global().write().unwrap() = Self::all();
    }

    /// Disable all debug flags
    ///
    /// Returns to minimal debugging output.
    pub fn disable_all() {
        *Self::global().write().unwrap() = Self::empty();
    }

    /// Set debug flags to exact value
    ///
    /// Replaces all current flags with the given set.
    pub fn set_global(flags: DebugFlags) {
        *Self::global().write().unwrap() = flags;
    }

    /// Get current debug flags
    ///
    /// Returns a copy of the current flag state.
    pub fn get_global() -> Self {
        *Self::global().read().unwrap()
    }
}

/// Macro to check if a debug flag is enabled and execute code
///
/// This macro only runs in debug builds (when `debug_assertions` is enabled).
/// In release builds, the code is completely removed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::debug_println;
///
/// debug_println!(PRINT_BUILD_SCOPE, "Building widget: {}", widget_name);
/// ```
#[macro_export]
macro_rules! debug_println {
    ($flag:ident, $($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            if $crate::debug::DebugFlags::is_enabled($crate::debug::DebugFlags::$flag) {
                println!($($arg)*);
            }
        }
    };
}

/// Macro to conditionally execute debug code
///
/// Only runs in debug builds when the specified flag is enabled.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::debug_exec;
///
/// debug_exec!(CHECK_ELEMENT_LIFECYCLE, {
///     validate_lifecycle_state();
/// });
/// ```
#[macro_export]
macro_rules! debug_exec {
    ($flag:ident, $code:block) => {
        #[cfg(debug_assertions)]
        {
            if $crate::debug::DebugFlags::is_enabled($crate::debug::DebugFlags::$flag) {
                $code
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_flags_empty() {
        let flags = DebugFlags::empty();
        assert!(!flags.contains(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(!flags.contains(DebugFlags::PRINT_MARK_NEEDS_BUILD));
        assert!(!flags.contains(DebugFlags::PRINT_LAYOUT));
    }

    #[test]
    fn test_debug_flags_all() {
        let flags = DebugFlags::all();
        assert!(flags.contains(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(flags.contains(DebugFlags::PRINT_MARK_NEEDS_BUILD));
        assert!(flags.contains(DebugFlags::PRINT_LAYOUT));
        assert!(flags.contains(DebugFlags::CHECK_ELEMENT_LIFECYCLE));
    }

    #[test]
    fn test_debug_flags_insert() {
        let mut flags = DebugFlags::empty();
        flags.insert(DebugFlags::PRINT_BUILD_SCOPE);

        assert!(flags.contains(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(!flags.contains(DebugFlags::PRINT_LAYOUT));
    }

    #[test]
    fn test_debug_flags_remove() {
        let mut flags = DebugFlags::all();
        flags.remove(DebugFlags::PRINT_BUILD_SCOPE);

        assert!(!flags.contains(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(flags.contains(DebugFlags::PRINT_LAYOUT));
    }

    #[test]
    fn test_debug_flags_toggle() {
        let mut flags = DebugFlags::empty();

        flags.toggle(DebugFlags::PRINT_BUILD_SCOPE);
        assert!(flags.contains(DebugFlags::PRINT_BUILD_SCOPE));

        flags.toggle(DebugFlags::PRINT_BUILD_SCOPE);
        assert!(!flags.contains(DebugFlags::PRINT_BUILD_SCOPE));
    }

    #[test]
    fn test_debug_flags_union() {
        let flags1 = DebugFlags::PRINT_BUILD_SCOPE;
        let flags2 = DebugFlags::PRINT_LAYOUT;
        let combined = flags1 | flags2;

        assert!(combined.contains(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(combined.contains(DebugFlags::PRINT_LAYOUT));
    }

    #[test]
    fn test_debug_flags_intersection() {
        let flags = DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_LAYOUT;
        let mask = DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_MARK_NEEDS_BUILD;
        let result = flags & mask;

        assert!(result.contains(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(!result.contains(DebugFlags::PRINT_LAYOUT));
        assert!(!result.contains(DebugFlags::PRINT_MARK_NEEDS_BUILD));
    }

    #[test]
    fn test_debug_flags_global() {
        // Reset to clean state
        DebugFlags::disable_all();

        // Enable some flags
        DebugFlags::enable(DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_LAYOUT);

        // Check via is_enabled
        assert!(DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(DebugFlags::is_enabled(DebugFlags::PRINT_LAYOUT));
        assert!(!DebugFlags::is_enabled(DebugFlags::PRINT_MARK_NEEDS_BUILD));

        // Disable specific flag
        DebugFlags::disable(DebugFlags::PRINT_BUILD_SCOPE);
        assert!(!DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(DebugFlags::is_enabled(DebugFlags::PRINT_LAYOUT));

        // Clean up
        DebugFlags::disable_all();
    }

    #[test]
    fn test_debug_flags_enable_all() {
        DebugFlags::disable_all();
        DebugFlags::enable_all();

        assert!(DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(DebugFlags::is_enabled(DebugFlags::PRINT_LAYOUT));
        assert!(DebugFlags::is_enabled(DebugFlags::CHECK_ELEMENT_LIFECYCLE));

        DebugFlags::disable_all();
    }

    #[test]
    fn test_debug_flags_set_global() {
        DebugFlags::disable_all();

        DebugFlags::set_global(DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_LAYOUT);

        assert!(DebugFlags::is_enabled(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(DebugFlags::is_enabled(DebugFlags::PRINT_LAYOUT));
        assert!(!DebugFlags::is_enabled(DebugFlags::PRINT_MARK_NEEDS_BUILD));

        DebugFlags::disable_all();
    }

    #[test]
    fn test_debug_flags_get_global() {
        DebugFlags::disable_all();
        DebugFlags::enable(DebugFlags::PRINT_BUILD_SCOPE);

        let flags = DebugFlags::get_global();
        assert!(flags.contains(DebugFlags::PRINT_BUILD_SCOPE));
        assert!(!flags.contains(DebugFlags::PRINT_LAYOUT));

        DebugFlags::disable_all();
    }

    #[test]
    fn test_debug_flags_bits() {
        // Test that each flag has a unique bit
        assert_eq!(DebugFlags::PRINT_BUILD_SCOPE.bits(), 1 << 0);
        assert_eq!(DebugFlags::PRINT_MARK_NEEDS_BUILD.bits(), 1 << 1);
        assert_eq!(DebugFlags::PRINT_LAYOUT.bits(), 1 << 2);
    }

    #[test]
    fn test_debug_flags_copy() {
        let flags1 = DebugFlags::PRINT_BUILD_SCOPE;
        let flags2 = flags1; // Copy

        assert_eq!(flags1, flags2);
    }

    #[test]
    fn test_debug_flags_clone() {
        let flags1 = DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_LAYOUT;
        let flags2 = flags1.clone();

        assert_eq!(flags1, flags2);
    }

    #[test]
    fn test_debug_flags_debug_format() {
        let flags = DebugFlags::PRINT_BUILD_SCOPE | DebugFlags::PRINT_LAYOUT;
        let debug_str = format!("{:?}", flags);

        // Should contain both flag names
        assert!(debug_str.contains("PRINT_BUILD_SCOPE"));
        assert!(debug_str.contains("PRINT_LAYOUT"));
    }
}
