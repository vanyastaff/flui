//! Readback of the actual swapchain image, before its present. This does not
//! observe physical display scanout and is never used as presentation feedback.

use parking_lot::Mutex;
use std::sync::Arc;

/// One pixel of the image sent to the native swapchain.
#[derive(Clone, Copy, Debug)]
pub struct SurfacePixel {
    /// Device-pixel column.
    pub x: u32,
    /// Device-pixel row.
    pub y: u32,
    /// Encoded surface bytes in RGBA channel order.
    pub rgba: [u8; 4],
}

/// Five distinguishable samples from one actual native surface image.
#[derive(Clone, Debug)]
pub struct SurfaceReadback {
    /// Actual copied swapchain extent in device pixels.
    pub size: (u32, u32),
    /// Four quadrant samples and one centre sample.
    pub pixels: [SurfacePixel; 5],
}

/// A readback could not establish the surface's pixels.
#[derive(Debug, thiserror::Error)]
pub enum SurfaceReadbackError {
    /// No supported copy operation/format on this surface.
    #[error("surface does not support the qualification's RGBA8/BGRA8 copy")]
    Unsupported,
    /// Packed RGBA pixels exceed 16 MiB. GPU row padding is additional;
    /// the staging buffer and packed CPU copy coexist during readback.
    #[error("surface exceeds the 16 MiB packed-pixel qualification limit")]
    Oversize,
    /// GPU copy, map or wait failed.
    #[error(transparent)]
    Engine(#[from] crate::EngineError),
}

#[derive(Default)]
struct State {
    requested: bool,
    result: Option<Result<SurfaceReadback, SurfaceReadbackError>>,
}

/// One pending capture and one unread result; repeated requests cannot allocate
/// an unbounded capture queue. GPU work and waits occur outside this state lock.
#[derive(Clone, Default)]
pub struct FrameObserver(Arc<Mutex<State>>);

impl FrameObserver {
    /// Request the next rendered image. Refuses while a request/result is held.
    pub fn request_capture(&self) -> bool {
        let mut state = self.0.lock();
        if state.requested || state.result.is_some() {
            return false;
        }
        state.requested = true;
        true
    }

    /// Take the previous requested capture, including a typed failure.
    pub fn take_capture(&self) -> Option<Result<SurfaceReadback, SurfaceReadbackError>> {
        self.0.lock().result.take()
    }
}

/// Observe this renderer's actual swapchain without installing a user callback.
pub fn observe_surface(renderer: &mut super::Renderer) -> FrameObserver {
    let observer = FrameObserver::default();
    renderer.qualification = Some(observer.clone());
    observer
}

pub(super) fn sample_requested_frame(renderer: &super::Renderer, texture: &wgpu::Texture) {
    let Some(observer) = &renderer.qualification else {
        return;
    };
    if !observer.0.lock().requested {
        return;
    }
    let result = sample(renderer, texture);
    let mut state = observer.0.lock();
    state.result = Some(result);
    state.requested = false;
}

fn sample(
    renderer: &super::Renderer,
    texture: &wgpu::Texture,
) -> Result<SurfaceReadback, SurfaceReadbackError> {
    let bgra = match texture.format() {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
        _ => return Err(SurfaceReadbackError::Unsupported),
    };
    if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
        return Err(SurfaceReadbackError::Unsupported);
    }
    let width = texture.width();
    let height = texture.height();
    if u64::from(width) * u64::from(height) > 4_194_304 {
        return Err(SurfaceReadbackError::Oversize);
    }
    let bytes = crate::headless::readback_rgba(renderer.painter.domain(), texture, width, height)?;
    let points = [
        (width / 4, height / 4),
        (width * 3 / 4, height / 4),
        (width / 2, height / 2),
        (width / 4, height * 3 / 4),
        (width * 3 / 4, height * 3 / 4),
    ];
    let pixels = points.map(|(x, y)| {
        let offset = (u64::from(y) * u64::from(width) + u64::from(x)) as usize * 4;
        let mut rgba = <[u8; 4]>::try_from(&bytes[offset..offset + 4])
            .expect("BUG: bounded pixel is within readback");
        if bgra {
            rgba.swap(0, 2);
        }
        SurfacePixel { x, y, rgba }
    });
    Ok(SurfaceReadback {
        size: (width, height),
        pixels,
    })
}
