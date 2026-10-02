//! Texture pooling for offscreen rendering
//!
//! Manages GPU texture allocation and reuse to minimize allocation overhead
//! during shader mask rendering. Textures are created via `wgpu::Device` and
//! returned to the pool on drop for reuse.
//!
//! # Ownership shape
//!
//! The pool's inventory is a plain, directly-owned value — no lock. What made
//! the previous `Arc<Mutex<TexturePoolInner>>` shape necessary was
//! return-on-drop: every [`PooledTexture`] held a back-reference into the
//! pool. That back-reference is now a lightweight mpsc [`Sender`]: dropping a
//! `PooledTexture` sends its texture down the channel, and the pool drains
//! the channel back into its inventory at the top of every `&mut self`
//! operation. A texture outliving its pool degrades gracefully — the failed
//! send just drops the GPU resource. The pool is therefore `Send` but not
//! `Sync` (single-mutator by construction, matching its actual use: one
//! renderer thread).

use std::sync::{
    Arc,
    mpsc::{Receiver, Sender, channel},
};

/// Texture descriptor key for matching pooled textures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TextureDesc {
    /// Width in pixels
    pub(crate) width: u32,
    /// Height in pixels
    pub(crate) height: u32,
    /// wgpu texture format
    pub(crate) format: wgpu::TextureFormat,
}

impl TextureDesc {
    /// Get total size in bytes (approximate)
    pub(crate) fn size_bytes(&self) -> usize {
        let bpp = self.format.block_copy_size(None).unwrap_or(4) as usize;
        (self.width as usize) * (self.height as usize) * bpp
    }
}

/// GPU texture with its view, managed by the pool
///
/// Holds ownership of a `wgpu::Texture` and a default `wgpu::TextureView`.
/// These are moved in and out of the pool — never cloned.
pub(crate) struct GpuTexture {
    /// The actual GPU texture
    pub(crate) texture: wgpu::Texture,
    /// Default texture view (created at allocation time)
    pub(crate) view: wgpu::TextureView,
    /// Descriptor used to create this texture (for matching)
    pub(crate) desc: TextureDesc,
}

// wgpu::Texture does not implement Debug
impl std::fmt::Debug for GpuTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuTexture")
            .field("desc", &self.desc)
            .finish_non_exhaustive()
    }
}

/// Handle to a pooled texture. Returns the texture to the pool on drop.
///
/// Access the underlying GPU texture and view via [`texture()`](Self::texture)
/// and [`view()`](Self::view).
pub(crate) struct PooledTexture {
    /// Inner GPU texture — `Option` so we can `take()` in Drop
    gpu_texture: Option<GpuTexture>,
    /// Return channel back to the pool for return-on-drop. Not a reference
    /// into the pool's inventory — the pool drains this channel under its own
    /// exclusive borrow.
    return_tx: Sender<GpuTexture>,
}

// Manual Debug because GpuTexture uses manual Debug; the return channel
// carries no printable state.
impl std::fmt::Debug for PooledTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledTexture")
            .field("desc", &self.desc())
            .field("has_texture", &self.gpu_texture.is_some())
            .finish_non_exhaustive()
    }
}

impl PooledTexture {
    /// Get texture descriptor
    pub(crate) fn desc(&self) -> &TextureDesc {
        &self
            .gpu_texture
            .as_ref()
            .expect("PooledTexture: gpu_texture taken before access")
            .desc
    }

    /// Get width in pixels
    pub(crate) fn width(&self) -> u32 {
        self.desc().width
    }

    /// Get height in pixels
    pub(crate) fn height(&self) -> u32 {
        self.desc().height
    }

    /// Get the underlying wgpu texture
    pub(crate) fn texture(&self) -> &wgpu::Texture {
        &self
            .gpu_texture
            .as_ref()
            .expect("PooledTexture: gpu_texture taken before access")
            .texture
    }

    /// Get the default texture view
    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self
            .gpu_texture
            .as_ref()
            .expect("PooledTexture: gpu_texture taken before access")
            .view
    }
}

impl Drop for PooledTexture {
    fn drop(&mut self) {
        if let Some(gpu_tex) = self.gpu_texture.take() {
            tracing::trace!("Returning texture to pool: {:?}", gpu_tex.desc);
            // A send failure means the pool itself is gone; the texture is
            // dropped right here, releasing the GPU resource.
            let _ = self.return_tx.send(gpu_tex);
        }
    }
}

/// Internal texture pool state
struct TexturePoolInner {
    /// Available (idle) textures keyed by descriptor
    available: Vec<GpuTexture>,
    /// Total number of textures ever allocated (including those currently out)
    total_allocated: usize,
    /// Maximum number of idle textures to keep in the pool
    max_pool_size: usize,
    /// Total memory used by all allocated textures (bytes)
    total_memory_bytes: usize,
}

// Manual Debug because GpuTexture uses manual Debug
impl std::fmt::Debug for TexturePoolInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TexturePoolInner")
            .field("available_count", &self.available.len())
            .field("total_allocated", &self.total_allocated)
            .field("max_pool_size", &self.max_pool_size)
            .field("total_memory_bytes", &self.total_memory_bytes)
            .finish()
    }
}

impl TexturePoolInner {
    fn new(max_pool_size: usize) -> Self {
        Self {
            available: Vec::new(),
            total_allocated: 0,
            max_pool_size,
            total_memory_bytes: 0,
        }
    }

    /// Try to find and remove a matching texture from the available pool
    fn take_matching(&mut self, desc: &TextureDesc) -> Option<GpuTexture> {
        if let Some(idx) = self.available.iter().position(|t| t.desc == *desc) {
            tracing::trace!("Texture pool hit: {:?}", desc);
            Some(self.available.swap_remove(idx))
        } else {
            None
        }
    }

    /// Return a texture to the pool for future reuse
    fn return_texture(&mut self, gpu_tex: GpuTexture) {
        if self.available.len() < self.max_pool_size {
            tracing::trace!("Texture returned to pool: {:?}", gpu_tex.desc);
            self.available.push(gpu_tex);
        } else {
            // Pool full — discard the texture (GPU resource dropped)
            self.total_allocated = self.total_allocated.saturating_sub(1);
            self.total_memory_bytes = self
                .total_memory_bytes
                .saturating_sub(gpu_tex.desc.size_bytes());
            tracing::trace!("Texture pool full, discarding: {:?}", gpu_tex.desc);
            // gpu_tex is dropped here, releasing the GPU resource
        }
    }
}

/// Texture pool for offscreen rendering — single-mutator, `Send`-only.
///
/// Manages allocation and reuse of GPU textures to minimize overhead.
/// Textures are created via `wgpu::Device::create_texture()` with
/// `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC` usage flags. The
/// inventory is directly owned (no lock — see the module doc); dropped
/// [`PooledTexture`]s come home through the return channel, drained at the
/// top of every `&mut self` operation.
///
/// # Example
///
/// `TexturePool` is crate-private, so this sketch shows the internal call
/// shape rather than compiling:
///
/// ```text
/// let mut pool = TexturePool::with_capacity(device.clone(), 16);
/// let texture = pool.acquire(800, 600, wgpu::TextureFormat::Rgba8UnormSrgb);
///
/// // Use texture.texture() and texture.view() for rendering...
///
/// // Texture automatically returned to pool when dropped
/// ```
// `missing_debug_implementations` is a crate-level `#[expect]`: these types
// hold `wgpu` handles, whose lack of `Debug` is the whole reason it exists.
pub(crate) struct TexturePool {
    inventory: TexturePoolInner,
    return_tx: Sender<GpuTexture>,
    return_rx: Receiver<GpuTexture>,
    device: Arc<wgpu::Device>,
}

impl TexturePool {
    /// Create texture pool with specific max pool size for idle textures
    pub(crate) fn with_capacity(device: Arc<wgpu::Device>, max_pool_size: usize) -> Self {
        let (return_tx, return_rx) = channel();
        Self {
            inventory: TexturePoolInner::new(max_pool_size),
            return_tx,
            return_rx,
            device,
        }
    }

    /// Move every texture waiting in the return channel back into the
    /// inventory. Called at the top of every `&mut self` operation so the
    /// inventory is current before it is read or taken from.
    fn drain_returns(&mut self) {
        while let Ok(gpu_tex) = self.return_rx.try_recv() {
            self.inventory.return_texture(gpu_tex);
        }
    }

    /// Acquire a texture from the pool (or create a new one)
    ///
    /// The returned [`PooledTexture`] automatically returns the GPU texture
    /// to the pool when dropped.
    #[must_use]
    pub(crate) fn acquire(
        &mut self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> PooledTexture {
        self.drain_returns();
        let desc = TextureDesc {
            width: width.max(1),
            height: height.max(1),
            format,
        };

        // Try to reuse an existing texture
        let gpu_texture = if let Some(existing) = self.inventory.take_matching(&desc) {
            existing
        } else {
            // Create a new GPU texture
            let gpu_tex = self.create_gpu_texture(&desc);
            self.inventory.total_allocated += 1;
            self.inventory.total_memory_bytes += desc.size_bytes();
            tracing::trace!(
                "Created new texture: {:?} (total: {}, memory: {} KB)",
                desc,
                self.inventory.total_allocated,
                self.inventory.total_memory_bytes / 1024
            );
            gpu_tex
        };

        PooledTexture {
            gpu_texture: Some(gpu_texture),
            return_tx: self.return_tx.clone(),
        }
    }

    /// Create a GPU texture matching the given descriptor
    fn create_gpu_texture(&self, desc: &TextureDesc) -> GpuTexture {
        let wgpu_desc = wgpu::TextureDescriptor {
            label: Some("TexturePool Offscreen"),
            size: wgpu::Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: desc.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };

        let texture = self.device.create_texture(&wgpu_desc);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        GpuTexture {
            texture,
            view,
            desc: *desc,
        }
    }
}
