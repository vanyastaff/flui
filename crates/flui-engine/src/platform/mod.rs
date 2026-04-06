//! Platform-specific GPU backend optimizations
//!
//! Optional platform-specific code paths for Metal, DX12, and Vulkan
//! that go beyond what wgpu abstracts.

#[cfg(target_os = "macos")]
pub mod metal;

#[cfg(target_os = "windows")]
pub mod dx12;

#[cfg(target_os = "linux")]
pub mod vulkan;
