//! GPU context management
//!
//! Owns the wgpu `Device`, `Queue`, `Surface`, and capability queries.
//! Replaces the monolithic device setup from `WgpuPainter`.

pub mod gpu_device;
pub mod render_surface;
pub mod capabilities;
pub mod headless;
