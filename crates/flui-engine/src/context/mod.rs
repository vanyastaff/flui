//! GPU context management
//!
//! Owns the wgpu `Device`, `Queue`, `Surface`, and capability queries.
//! Replaces the monolithic device setup from `WgpuPainter`.

#[cfg(feature = "wgpu-backend")]
pub mod capabilities;
#[cfg(feature = "wgpu-backend")]
pub mod gpu_device;
#[cfg(feature = "wgpu-backend")]
pub mod render_surface;

pub mod headless;
