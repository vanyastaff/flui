//! Debug infrastructure for Flui
//!
//! This module provides debug flags, diagnostic tools, and validation infrastructure
//! for development and debugging.
//!
//! # Debug Flags
//!
//! Global debug flags control logging and validation:
//!
//! ```rust,ignore
//! use flui_core::debug::DebugFlags;
//!
//! // Enable debug logging
//! DebugFlags::global().write().debug_print_build_scope = true;
//! ```
//!
//! # Submodules
//!
//! - `diagnostics` - Element tree diagnostic printing
//! - `lifecycle` - Lifecycle validation
//! - `key_registry` - Global key uniqueness validation

use std::sync::RwLock;

pub mod diagnostics;
pub mod key_registry;
pub mod lifecycle;


/// Global debug flags for controlling debug output
///
/// These flags are checked at runtime to control debug logging and validation.
/// In release builds, these checks should be optimized away by the compiler when
/// wrapped in `#[cfg(debug_assertions)]`.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::debug::DebugFlags;
///
/// // Enable debug printing
/// {
///     let mut flags = DebugFlags::global().write().unwrap();
///     flags.debug_print_build_scope = true;
///     flags.debug_print_mark_needs_build = true;
/// }
///
/// // Now widget builds will be logged
/// ```
#[derive(Debug, Clone)]
pub struct DebugFlags {
    /// Print when build() is called on widgets
    pub debug_print_build_scope: bool,

    /// Print when mark_needs_build() is called
    pub debug_print_mark_needs_build: bool,

    /// Print when layout() is called on RenderObjects
    pub debug_print_layout: bool,

    /// Print when rebuild is scheduled
    pub debug_print_schedule_build: bool,

    /// Print global key registration/deregistration
    pub debug_print_global_key_registry: bool,

    /// Enable element lifecycle validation
    pub debug_check_element_lifecycle: bool,

    /// Enable intrinsic size validation
    pub debug_check_intrinsic_sizes: bool,

    /// Print when InheritedWidget notifies dependents
    pub debug_print_inherited_widget_notify: bool,

    /// Print when dependencies are registered
    pub debug_print_dependencies: bool,
}

impl Default for DebugFlags {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugFlags {
    /// Create new debug flags with all flags disabled
    pub fn new() -> Self {
        Self {
            debug_print_build_scope: false,
            debug_print_mark_needs_build: false,
            debug_print_layout: false,
            debug_print_schedule_build: false,
            debug_print_global_key_registry: false,
            debug_check_element_lifecycle: false,
            debug_check_intrinsic_sizes: false,
            debug_print_inherited_widget_notify: false,
            debug_print_dependencies: false,
        }
    }

    /// Create debug flags with all flags enabled
    pub fn all() -> Self {
        Self {
            debug_print_build_scope: true,
            debug_print_mark_needs_build: true,
            debug_print_layout: true,
            debug_print_schedule_build: true,
            debug_print_global_key_registry: true,
            debug_check_element_lifecycle: true,
            debug_check_intrinsic_sizes: true,
            debug_print_inherited_widget_notify: true,
            debug_print_dependencies: true,
        }
    }

    /// Get global debug flags instance (thread-local)
    pub fn global() -> &'static RwLock<Self> {
        static INSTANCE: RwLock<DebugFlags> = RwLock::new(DebugFlags {
            debug_print_build_scope: false,
            debug_print_mark_needs_build: false,
            debug_print_layout: false,
            debug_print_schedule_build: false,
            debug_print_global_key_registry: false,
            debug_check_element_lifecycle: false,
            debug_check_intrinsic_sizes: false,
            debug_print_inherited_widget_notify: false,
            debug_print_dependencies: false,
        });
        &INSTANCE
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
/// debug_println!(debug_print_build_scope, "Building widget: {}", widget_name);
/// ```
#[macro_export]
macro_rules! debug_println {
    ($flag:ident, $($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            if $crate::debug::DebugFlags::global().read().unwrap().$flag {
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
/// debug_exec!(debug_check_element_lifecycle, {
///     validate_lifecycle_state();
/// });
/// ```
#[macro_export]
macro_rules! debug_exec {
    ($flag:ident, $code:block) => {
        #[cfg(debug_assertions)]
        {
            if $crate::debug::DebugFlags::global().read().unwrap().$flag {
                $code
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_flags_new() {
        let flags = DebugFlags::new();
        assert!(!flags.debug_print_build_scope);
        assert!(!flags.debug_print_mark_needs_build);
        assert!(!flags.debug_print_layout);
        assert!(!flags.debug_check_element_lifecycle);
    }

    #[test]
    fn test_debug_flags_all() {
        let flags = DebugFlags::all();
        assert!(flags.debug_print_build_scope);
        assert!(flags.debug_print_mark_needs_build);
        assert!(flags.debug_print_layout);
        assert!(flags.debug_check_element_lifecycle);
    }

    #[test]
    fn test_debug_flags_global() {
        // Test that global instance is accessible
        let flags = DebugFlags::global();
        assert!(flags.read().is_ok());
    }

    #[test]
    fn test_debug_flags_global_modify() {
        // Modify global flags
        {
            let mut flags = DebugFlags::global().write().unwrap();
            flags.debug_print_build_scope = true;
        }

        // Read back
        {
            let flags = DebugFlags::global().read().unwrap();
            assert!(flags.debug_print_build_scope);
        }

        // Reset for other tests
        {
            let mut flags = DebugFlags::global().write().unwrap();
            flags.debug_print_build_scope = false;
        }
    }

    #[test]
    fn test_debug_flags_default() {
        let flags = DebugFlags::default();
        assert!(!flags.debug_print_build_scope);
        assert!(!flags.debug_print_inherited_widget_notify);
    }

    #[test]
    fn test_debug_flags_clone() {
        let flags1 = DebugFlags::all();
        let flags2 = flags1.clone();

        assert_eq!(flags1.debug_print_build_scope, flags2.debug_print_build_scope);
        assert_eq!(flags1.debug_check_element_lifecycle, flags2.debug_check_element_lifecycle);
    }
}



