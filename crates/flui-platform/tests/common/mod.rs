//! Common test utilities for flui-platform
//!
//! This module provides shared utilities for writing tests across
//! all platform implementations.

pub mod contract_framework;

// Re-export commonly used items
pub use contract_framework::{ContractTest, PlatformContract, ContractResult};
