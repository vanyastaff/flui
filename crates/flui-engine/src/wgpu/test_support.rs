//! Shared GPU scaffolding for the `enable-wgpu-tests` suites.
//!
//! Before this module existed, every GPU test file carried its own copy of the
//! same four rituals — adapter/device acquisition, render-target creation, a
//! clear pass, and the padded-row staging readback — under six different names
//! (`acquire_test_device_and_queue`, `test_device_and_queue`, `device_queue`,
//! …). The copies drifted only in labels and expect-message wording, and every
//! wgpu API migration (most recently wgpu 30's `apply_limit_buckets` and
//! fallible `get_mapped_range`) had to be applied to each copy by hand. This
//! module is the single home for those rituals; per-file oracles, painters,
//! and scene builders stay local to their suites.
//!
//! Two device flavours exist on purpose:
//! - [`test_device_and_queue`] / [`test_device`] panic when no adapter is
//!   present — for suites that only run on GPU-enabled hosts.
//! - [`try_test_device_and_queue`] returns `None` instead — for suites whose
//!   tests self-skip on hosts without a usable adapter.
//!
//! Not served here: `profiler.rs`'s acquisition (it negotiates timestamp-query
//! features and inspects the adapter, a genuinely different contract).

use std::sync::Arc;

/// Requests the low-power test adapter every GPU suite uses.
///
/// `apply_limit_buckets: false` — wgpu 30's anti-fingerprinting knob is for
/// embedders exposing a GPU to untrusted content; tests want the adapter's
/// real limits. Same answer as the production sites in
/// [`super::adapter::trusted_adapter_options`].
fn request_test_adapter() -> Result<wgpu::Adapter, wgpu::RequestAdapterError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
}

fn request_device(adapter: &wgpu::Adapter, label: &str) -> (wgpu::Device, wgpu::Queue) {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        ..Default::default()
    }))
    .expect("GPU device creation succeeded when adapter was found")
}

/// Acquires the shared test device + queue, panicking when no adapter exists.
///
/// `label` names the device in wgpu validation errors — pass the suite name.
pub(crate) fn test_device_and_queue(label: &str) -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
    let adapter =
        request_test_adapter().expect("a GPU adapter must be available on a GPU-enabled test host");
    let (device, queue) = request_device(&adapter, label);
    (Arc::new(device), Arc::new(queue))
}

/// Like [`test_device_and_queue`], but yields `None` when no adapter exists so
/// callers can self-skip: `let Some((device, queue)) = … else { return; };`
pub(crate) fn try_test_device_and_queue(
    label: &str,
) -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    let adapter = request_test_adapter().ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        ..Default::default()
    }))
    .ok()?;
    Some((Arc::new(device), Arc::new(queue)))
}

/// Device-only variant for construction tests that never submit work.
pub(crate) fn test_device(label: &str) -> wgpu::Device {
    let (device, _queue) = request_device(
        &request_test_adapter()
            .expect("a GPU adapter must be available on a GPU-enabled test host"),
        label,
    );
    device
}

/// Creates a 2D single-sample render target with the given usage set.
pub(crate) fn create_target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// [`create_target`] with the sampleable-attachment usage set the filter and
/// blend suites need (`RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC |
/// COPY_DST`).
pub(crate) fn create_sampleable_target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    create_target(
        device,
        label,
        width,
        height,
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
    )
}

/// Clears `view` to `color` with a standalone submitted render pass.
pub(crate) fn clear_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    color: wgpu::Color,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("test_support clear encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test_support clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    queue.submit(std::iter::once(encoder.finish()));
}

/// Reads `width × height` texels back as tightly packed RGBA8 bytes.
///
/// Handles the `COPY_BYTES_PER_ROW_ALIGNMENT` row padding, waits for the map
/// with `PollType::Wait`, and dumps the frame via [`super::readback_dump`]
/// (a no-op unless `FLUI_READBACK_DUMP_DIR` is set). The texture must use a
/// 4-byte-per-texel format and have `COPY_SRC` usage.
pub(crate) fn readback_bytes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let bytes_per_pixel = 4u32;
    let unpadded_row_bytes = width * bytes_per_pixel;
    let row_alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_row_bytes = unpadded_row_bytes.div_ceil(row_alignment) * row_alignment;
    let staging_size = u64::from(padded_row_bytes * height);

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test_support readback staging"),
        size: staging_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("test_support readback encoder"),
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
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("GPU readback poll must complete within wait timeout");

    let raw_bytes = staging
        .slice(..)
        .get_mapped_range()
        .expect("staging buffer must be mapped: the poll above waited for the map to complete");
    let mut bytes = Vec::with_capacity((unpadded_row_bytes * height) as usize);
    for row_index in 0..height {
        let row_start = (row_index * padded_row_bytes) as usize;
        bytes.extend_from_slice(&raw_bytes[row_start..row_start + unpadded_row_bytes as usize]);
    }
    drop(raw_bytes);
    staging.unmap();

    super::readback_dump::dump_frame(width, height, &bytes);
    bytes
}

/// Reads `width × height` texels back as `[r, g, b, a]` u8 quads.
pub(crate) fn readback_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<[u8; 4]> {
    readback_bytes(device, queue, texture, width, height)
        .as_chunks::<4>()
        .0
        .to_vec()
}
