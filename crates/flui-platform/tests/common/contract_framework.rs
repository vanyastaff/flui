//! Contract Test Framework for Platform Trait Compliance
//!
//! This module provides utilities for writing contract tests that ensure all platform
//! implementations (Windows, macOS, Headless, etc.) provide identical API behavior.
//!
//! # Contract Testing Philosophy
//!
//! Contract tests verify that:
//! - All platforms implement the same trait methods
//! - Methods return values in expected ranges/formats
//! - Error cases are handled consistently
//! - Platform-specific behavior falls within defined contracts
//!
//! # Usage
//!
//! ```rust,ignore
//! use common::contract_framework::{ContractTest, PlatformContract};
//!
//! fn test_my_feature() {
//!     let mut contract = PlatformContract::new();
//!
//!     contract.test("feature_name", |platform| {
//!         // Test platform-agnostic behavior
//!         assert!(platform.some_method());
//!         Ok(())
//!     });
//!
//!     contract.run();
//! }
//! ```

use flui_platform::Platform;
use std::sync::Arc;

/// Result type for contract tests
pub type ContractResult = Result<(), Box<dyn std::error::Error>>;

/// A single contract test case
pub struct ContractTest {
    name: String,
    test_fn: Box<dyn Fn(&Arc<dyn Platform>) -> ContractResult>,
    skip_on_headless: bool,
}

impl ContractTest {
    /// Create a new contract test
    pub fn new<F>(name: impl Into<String>, test_fn: F) -> Self
    where
        F: Fn(&Arc<dyn Platform>) -> ContractResult + 'static,
    {
        Self {
            name: name.into(),
            test_fn: Box::new(test_fn),
            skip_on_headless: false,
        }
    }

    /// Mark this test as needing to be skipped on headless platforms
    pub fn skip_on_headless(mut self) -> Self {
        self.skip_on_headless = true;
        self
    }

    /// Run the contract test on a given platform
    pub fn run(&self, platform: &Arc<dyn Platform>) -> ContractResult {
        let platform_name = platform.name();

        if self.skip_on_headless && platform_name == "Headless" {
            tracing::info!("⊘ SKIP: {} (headless not applicable)", self.name);
            return Ok(());
        }

        tracing::info!("→ Testing contract: {} on {}", self.name, platform_name);

        match (self.test_fn)(platform) {
            Ok(()) => {
                tracing::info!("✓ PASS: {}", self.name);
                Ok(())
            }
            Err(e) => {
                tracing::error!("✗ FAIL: {} - {}", self.name, e);
                Err(e)
            }
        }
    }
}

/// A collection of contract tests for platform compliance
pub struct PlatformContract {
    tests: Vec<ContractTest>,
}

impl PlatformContract {
    /// Create a new platform contract test suite
    pub fn new() -> Self {
        Self { tests: Vec::new() }
    }

    /// Add a contract test
    pub fn add_test(&mut self, test: ContractTest) {
        self.tests.push(test);
    }

    /// Add a contract test with a closure
    pub fn test<F>(&mut self, name: impl Into<String>, test_fn: F) -> &mut Self
    where
        F: Fn(&Arc<dyn Platform>) -> ContractResult + 'static,
    {
        self.tests.push(ContractTest::new(name, test_fn));
        self
    }

    /// Add a contract test that skips headless platforms
    pub fn test_skip_headless<F>(&mut self, name: impl Into<String>, test_fn: F) -> &mut Self
    where
        F: Fn(&Arc<dyn Platform>) -> ContractResult + 'static,
    {
        self.tests
            .push(ContractTest::new(name, test_fn).skip_on_headless());
        self
    }

    /// Run all contract tests on the current platform
    pub fn run(&self) {
        let platform = flui_platform::current_platform().expect("Failed to get platform");
        let platform_name = platform.name();

        tracing::info!("═══════════════════════════════════════");
        tracing::info!("Running {} contract tests on {}", self.tests.len(), platform_name);
        tracing::info!("═══════════════════════════════════════");

        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for test in &self.tests {
            match test.run(&platform) {
                Ok(()) => {
                    if test.skip_on_headless && platform_name == "Headless" {
                        skipped += 1;
                    } else {
                        passed += 1;
                    }
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }

        tracing::info!("═══════════════════════════════════════");
        tracing::info!(
            "Results: {} passed, {} failed, {} skipped",
            passed,
            failed,
            skipped
        );
        tracing::info!("═══════════════════════════════════════");

        assert_eq!(failed, 0, "Some contract tests failed");
    }
}

impl Default for PlatformContract {
    fn default() -> Self {
        Self::new()
    }
}

/// Macro to assert a contract condition with context
#[macro_export]
macro_rules! contract_assert {
    ($cond:expr, $msg:expr) => {
        if !$cond {
            return Err(format!("Contract violation: {}", $msg).into());
        }
    };
    ($cond:expr, $fmt:expr, $($arg:tt)*) => {
        if !$cond {
            return Err(format!("Contract violation: {}", format!($fmt, $($arg)*)).into());
        }
    };
}

/// Macro to assert equality with context
#[macro_export]
macro_rules! contract_assert_eq {
    ($left:expr, $right:expr, $msg:expr) => {
        if $left != $right {
            return Err(format!(
                "Contract violation: {} (left: {:?}, right: {:?})",
                $msg, $left, $right
            )
            .into());
        }
    };
}

/// Common contract checks that apply to all platforms
pub mod common_contracts {
    use super::*;

    /// Contract: Platform must have a non-empty name
    pub fn has_valid_name(platform: &Arc<dyn Platform>) -> ContractResult {
        let name = platform.name();
        contract_assert!(!name.is_empty(), "Platform name must not be empty");
        Ok(())
    }

    /// Contract: Platform must provide display enumeration
    pub fn can_enumerate_displays(platform: &Arc<dyn Platform>) -> ContractResult {
        let displays = platform.displays();
        // Empty is OK for headless, but must return a valid collection
        contract_assert!(
            displays.len() <= 16,
            "Display count should be reasonable (<= 16)"
        );
        Ok(())
    }

    /// Contract: Display scale factors must be positive and reasonable
    pub fn has_valid_display_scales(platform: &Arc<dyn Platform>) -> ContractResult {
        let displays = platform.displays();
        for (idx, display) in displays.iter().enumerate() {
            let scale = display.scale_factor();
            contract_assert!(
                scale > 0.0 && scale <= 4.0,
                format!("Display {} scale factor should be 0.0-4.0, got {}", idx, scale)
            );
        }
        Ok(())
    }

    /// Contract: Clipboard API must not panic
    pub fn has_safe_clipboard_api(platform: &Arc<dyn Platform>) -> ContractResult {
        let clipboard = platform.clipboard();

        // Write and read should not panic
        clipboard.write_text("contract test".to_string());
        let _ = clipboard.read_text();

        Ok(())
    }

    /// Contract: Text system must be available
    pub fn has_text_system(platform: &Arc<dyn Platform>) -> ContractResult {
        let text_system = platform.text_system();
        let default_font = text_system.default_font_family();

        contract_assert!(
            !default_font.is_empty(),
            "Default font family must not be empty"
        );

        Ok(())
    }

    /// Contract: Executors must be available
    pub fn has_executors(platform: &Arc<dyn Platform>) -> ContractResult {
        let _bg = platform.background_executor();
        let _fg = platform.foreground_executor();

        // Just verify they don't panic on creation
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contract_framework() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let mut contract = PlatformContract::new();

        contract.test("platform_has_name", |platform| {
            contract_assert!(!platform.name().is_empty(), "Platform must have name");
            Ok(())
        });

        contract.test("can_get_displays", |platform| {
            let _displays = platform.displays();
            Ok(())
        });

        contract.run();
    }
}
