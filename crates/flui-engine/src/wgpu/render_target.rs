//! `RenderTarget` — the write destination for one render pass.
//!
//! A `RenderTarget` bundles the mandatory `wgpu::TextureView` (the attachment
//! written by every render pass) with an optional `wgpu::Texture` back-reference
//! (needed when a future pass wants to sample the surface as a backdrop — the
//! dst-read pattern required by advanced blend modes).
//!
//! ## Design constraints
//!
//! - Frame-scoped borrows: `RenderTarget<'a>` holds no ownership.  It is a
//!   **parameter**, not a stored field; it must never appear in `DrawItem`,
//!   `DrawSegment`, or any other IR type.
//! - `Copy + Clone`: callers thread it through nested flush calls without extra
//!   ceremony.
//! - The `texture` field is `None` for write-only targets (readback helpers,
//!   offscreen child paints) that are never sampled back.  Downstream passes
//!   that require backdrop sampling must call `RenderTarget::sampleable`.

/// The surface (or a pooled offscreen) the current pass writes to, plus an
/// optional back-reference to the underlying [`wgpu::Texture`] for passes that
/// need to sample it as a backdrop.
///
/// `RenderTarget<'a>` is a lightweight, frame-scoped parameter — it carries no
/// ownership and must **not** be stored inside any IR type.
#[derive(Clone, Copy)]
pub(crate) struct RenderTarget<'a> {
    /// The `TextureView` passed to `RenderPassColorAttachment::view`.
    pub(crate) view: &'a wgpu::TextureView,
    /// The backing `Texture`, present when a later pass is allowed to sample
    /// this target as a backdrop (dst-read blend modes).  `None` for purely
    /// write-only targets (readback helpers, offscreen child renders).
    ///
    /// Read by the dst-read blend pass when sampling the backdrop region;
    /// `None` targets cannot be sampled and must not be used with advanced modes.
    pub(crate) texture: Option<&'a wgpu::Texture>,
}

impl<'a> RenderTarget<'a> {
    /// Construct a target that may be sampled back by a later pass.
    ///
    /// Use this for the frame surface and any offscreen texture whose
    /// content a subsequent blend pass must read.
    #[inline]
    pub(crate) fn sampleable(view: &'a wgpu::TextureView, texture: &'a wgpu::Texture) -> Self {
        Self {
            view,
            texture: Some(texture),
        }
    }

    /// Construct a write-only target — no backdrop sampling allowed.
    ///
    /// Use this for readback helpers and offscreen child paints that
    /// are never read back by a blend shader.
    #[inline]
    pub(crate) fn view_only(view: &'a wgpu::TextureView) -> Self {
        Self {
            view,
            texture: None,
        }
    }
}
