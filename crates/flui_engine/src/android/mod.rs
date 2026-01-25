//! Android platform-specific functionality
//!
//! This module provides Android-specific features required for proper operation
//! on Android devices, particularly for API 35+ (Android 16) compliance.
//!
//! # Key Features
//!
//! - **16KB Page Size Support:** Memory allocators for 16KB page size devices
//! - **Vulkan Optimization:** Android-specific Vulkan configuration
//! - **Play Store Compliance:** Helpers for meeting Play Store requirements
//!
//! # Usage
//!
//! ```rust,ignore
//! #[cfg(target_os = "android")]
//! use flui_engine::android::memory::PageAlignedVec;
//!
//! #[cfg(target_os = "android")]
//! let buffer = PageAlignedVec::<u8>::with_capacity(8192);
//! ```

pub mod memory;

// Re-export commonly used types
pub use memory::{
    align_to_page_size, align_to_page_size_u64, get_page_size, is_16kb_page_size,
    PageAlignedVec,
};
