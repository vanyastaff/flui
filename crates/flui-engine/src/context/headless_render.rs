//! Headless render-to-texture infrastructure.
//!
//! Provides pixel readback from GPU textures for testing and off-screen
//! rendering scenarios. Used primarily with [`GpuDevice::create_render_texture`].

use crate::context::gpu_device::GpuDevice;

/// Read the contents of a GPU texture back to CPU as RGBA pixel data.
///
/// Copies the texture into a staging buffer, maps it, and strips wgpu's
/// 256-byte row padding to return a tightly-packed pixel buffer.
///
/// # Returns
/// A `Vec<u8>` of length `width * height * 4` containing pixel data in
/// the texture's native format (typically BGRA8).
pub fn read_texture_to_rgba(
    gpu: &GpuDevice,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let bytes_per_pixel = 4u32;
    // wgpu requires rows aligned to 256 bytes (COPY_BYTES_PER_ROW_ALIGNMENT)
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let padded_bytes_per_row = (unpadded_bytes_per_row + 255) & !255;
    let buffer_size = (padded_bytes_per_row * height) as u64;

    let staging = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback_staging"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback_encoder"),
        });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    gpu.queue().submit(std::iter::once(encoder.finish()));

    // Map buffer and read pixels back to CPU
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = gpu.device().poll(wgpu::PollType::Wait);
    // Safety: poll(Wait) guarantees the mapping is complete
    rx.recv()
        .expect("map_async callback was dropped")
        .expect("buffer mapping failed");

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * bytes_per_pixel) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + (width * bytes_per_pixel) as usize;
        pixels.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    staging.unmap();

    pixels
}
