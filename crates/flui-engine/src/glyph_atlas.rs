//! The glyph atlas: rasterised glyph bitmaps packed into two GPU pages.
//!
//! The engine does not shape text and does not rasterise it either: a
//! paragraph arrives as a [`flui_painting::ShapedParagraph`] the recorder
//! shaped and measured (ADR-0065, ADR-0092 §4), and every glyph bitmap comes
//! from the atlas's [`GlyphRasterizer`], keyed by the key each run places
//! (ADR-0067). The engine's atlas is a [`TextAtlas`]: swash drawing the faces
//! the paragraphs' runs carry, which the atlas's own registry keeps alive, so
//! rasterization takes no lock and shares no state with any realm. What the
//! engine owns is the cache: where each bitmap sits, how long it stays, and
//! the bind group the glyph pipeline samples it through.
//!
//! A rasterizer is trusted to be deterministic, but not to the point of a
//! GPU validation panic: [`GlyphImage`] guarantees valid CPU bitmap storage,
//! device-sized admission belongs here, and a grow re-uploads only images
//! that still fit the slot the first one sized.
//!
//! Two pages, because a coverage mask and a colour bitmap need different
//! texel formats: `R8Unorm` for the mask page the fragment shader tints, and
//! `Rgba8Unorm` for emoji and colour bitmap faces drawn as-is. Both live in
//! the engine's gamma-space convention (the surface is `Unorm`; see
//! `Renderer::select_surface_format`), so a glyph's colour lands as recorded.
//!
//! # Lifetime of a slot
//!
//! A slot is claimed the first frame its glyph is drawn and stamped with the
//! frame counter on every later use. Space is reclaimed lazily: when a page
//! cannot fit a new glyph, every slot not stamped THIS frame is freed and the
//! allocation retried; only if that still fails does the page grow (doubling,
//! up to the device's texture limit), re-uploading its live glyphs at the
//! positions they already hold — so an instance recorded before the grow
//! still points at the right texels. [`GlyphAtlas::end_frame`] advances the
//! counter once per presented frame; it never touches the texture a frame in
//! flight is sampling.

use std::sync::Arc;

use etagere::{AllocId, BucketedAtlasAllocator, size2};
use flui_painting::glyphs::SwashRasterizer;
use flui_painting::{GlyphContent, GlyphImage, GlyphRasterizer};
use rustc_hash::FxHashMap;

/// Where a glyph's bitmap sits in the atlas, and how it hangs off its origin.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlyphSlot {
    /// Texel origin in the page.
    pub texel: [u32; 2],
    /// Bitmap size in texels; `[0, 0]` for a glyph that draws nothing.
    pub size: [u32; 2],
    /// Horizontal bearing: bitmap left edge relative to the origin column.
    pub left: i32,
    /// Vertical bearing: bitmap top edge above the baseline row.
    pub top: i32,
    /// `true` when the bitmap is on the colour page.
    pub color_page: bool,
}

impl GlyphSlot {
    /// Whether the glyph has any texels to draw.
    pub(crate) fn is_empty(&self) -> bool {
        self.size[0] == 0 || self.size[1] == 0
    }
}

/// One slot's bookkeeping: the allocation it holds and when it was last used.
struct Entry {
    slot: GlyphSlot,
    /// `None` for an empty glyph, which occupies no atlas space.
    alloc: Option<AllocId>,
    last_used: u64,
}

/// One texture page and its packer.
struct Page {
    format: wgpu::TextureFormat,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    packer: BucketedAtlasAllocator,
    size: u32,
    /// Whether this is the colour page (the key's `color_page` selects it).
    is_color: bool,
    /// Allocations of glyphs a grow dropped while this frame still drew
    /// them; freed at the frame's end so no other glyph lands in a region a
    /// recorded draw samples.
    retired: Vec<AllocId>,
}

impl Page {
    const INITIAL_SIZE: u32 = 256;

    fn new(device: &wgpu::Device, is_color: bool) -> Self {
        let (format, label) = if is_color {
            (wgpu::TextureFormat::Rgba8Unorm, "Glyph Atlas (color)")
        } else {
            (wgpu::TextureFormat::R8Unorm, "Glyph Atlas (mask)")
        };
        let size = Self::INITIAL_SIZE.min(device.limits().max_texture_dimension_2d);
        let texture = create_page_texture(device, format, size, label);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            format,
            texture,
            view,
            packer: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            size,
            is_color,
            retired: Vec::new(),
        }
    }

    /// Frees the allocations a grow retired during the frame that just ended.
    fn release_retired(&mut self) {
        for alloc in self.retired.drain(..) {
            self.packer.deallocate(alloc);
        }
    }

    fn label(&self) -> &'static str {
        if self.is_color {
            "Glyph Atlas (color)"
        } else {
            "Glyph Atlas (mask)"
        }
    }

    fn bytes_per_texel(&self) -> u32 {
        match self.format {
            wgpu::TextureFormat::R8Unorm => 1,
            _ => 4,
        }
    }

    fn upload(&self, queue: &wgpu::Queue, texel: [u32; 2], size: [u32; 2], data: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: texel[0],
                    y: texel[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * self.bytes_per_texel()),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
    }

    /// Frees every slot of this page not used in `frame`. Returns how many
    /// were freed.
    fn evict_unused<K>(&mut self, entries: &mut FxHashMap<K, Entry>, frame: u64) -> usize {
        let packer = &mut self.packer;
        let is_color = self.is_color;
        let before = entries.len();
        entries.retain(|_, entry| {
            if entry.slot.color_page != is_color || entry.last_used == frame {
                return true;
            }
            if let Some(alloc) = entry.alloc {
                packer.deallocate(alloc);
            }
            false
        });
        before - entries.len()
    }

    /// Doubles the page, re-uploading every live glyph at its existing
    /// position. `false` when the page is already at the device limit.
    ///
    /// A glyph that cannot be re-rasterized, or an image that no longer fits
    /// its slot (another size or content kind) is not uploaded,
    /// which would fail wgpu's copy validation. Its entry is dropped, so the
    /// glyph's next use asks the rasterizer again, as after a `None`; its
    /// allocation is freed now, or at the frame's end if `frame` drew it.
    /// Warned once per grow.
    fn grow<R: GlyphRasterizer>(
        &mut self,
        entries: &mut FxHashMap<R::Key, Entry>,
        frame: u64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rasterizer: &mut R,
    ) -> bool {
        let max = device.limits().max_texture_dimension_2d;
        if self.size >= max {
            return false;
        }
        let new_size = (self.size * 2).min(max);
        self.packer.grow(size2(new_size as i32, new_size as i32));
        self.texture = create_page_texture(device, self.format, new_size, self.label());
        self.view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.size = new_size;

        // Re-rasterise rather than keep a CPU copy of every bitmap: a grow is
        // rare and the rasterizer is deterministic for a key.
        let content = if self.is_color {
            GlyphContent::Color
        } else {
            GlyphContent::Mask
        };
        let mut mismatched = Vec::new();
        for (key, entry) in entries.iter() {
            if entry.slot.color_page != self.is_color || entry.slot.is_empty() {
                continue;
            }
            let Some(image) = rasterizer.rasterize(*key) else {
                mismatched.push(*key);
                continue;
            };
            if [image.width(), image.height()] != entry.slot.size || image.content() != content {
                mismatched.push(*key);
                continue;
            }
            self.upload(queue, entry.slot.texel, entry.slot.size, image.data());
        }
        if let Some(key) = mismatched.first() {
            tracing::warn!(
                ?key,
                count = mismatched.len(),
                page = self.label(),
                "a glyph could not reproduce its bitmap on atlas grow; it is dropped and asked for again on its next use"
            );
        }
        for key in mismatched {
            let Some(Entry {
                alloc: Some(alloc),
                last_used,
                ..
            }) = entries.remove(&key)
            else {
                continue;
            };
            if last_used == frame {
                self.retired.push(alloc);
            } else {
                self.packer.deallocate(alloc);
            }
        }
        true
    }
}

fn create_page_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: u32,
    label: &'static str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The atlas paragraphs are recorded against: swash over the faces their
/// runs carry.
pub(crate) type TextAtlas = GlyphAtlas<SwashRasterizer>;

/// The rasterised-glyph cache the glyph pipeline samples.
///
/// Generic over where bitmaps come from: the engine's is a [`TextAtlas`],
/// the tests' a scripted rasterizer.
pub(crate) struct GlyphAtlas<R: GlyphRasterizer> {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    /// Where bitmaps come from. Owned by the atlas and taken by `&mut`.
    rasterizer: R,
    mask: Page,
    color: Page,
    /// Every glyph on either page, keyed for the record-time lookup.
    entries: FxHashMap<R::Key, Entry>,
    sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    /// The current frame; slots used this frame are immune to eviction.
    frame: u64,
    /// Whether a glyph was dropped this frame because a page could not grow.
    reported_full: bool,
}

impl<R: GlyphRasterizer> GlyphAtlas<R> {
    pub(crate) fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        layout: &wgpu::BindGroupLayout,
        rasterizer: R,
    ) -> Self {
        let mask = Page::new(&device, false);
        let color = Page::new(&device, true);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Glyph Atlas Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = create_bind_group(&device, layout, &mask, &color, &sampler);
        Self {
            device,
            queue,
            rasterizer,
            mask,
            color,
            entries: FxHashMap::default(),
            sampler,
            layout: layout.clone(),
            bind_group,
            frame: 0,
            reported_full: false,
        }
    }

    /// The rasterizer, so a recorder can hand it the faces a run names
    /// before asking for the run's glyphs.
    pub(crate) fn rasterizer_mut(&mut self) -> &mut R {
        &mut self.rasterizer
    }

    /// The bind group the glyph pipeline samples the two pages through.
    ///
    /// Valid for the frame it is read in: a grow rebuilds it, so a render
    /// pass must take it after the frame's recording is complete — which is
    /// the replay's order (record everything, then flush).
    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// Where `key`'s bitmap sits, rasterising and uploading it on first use.
    ///
    /// `None` when the glyph cannot be placed: the rasterizer cannot draw the
    /// key, or the page is at the device limit and every slot is in use this
    /// frame (reported once per frame). The glyph is skipped and nothing is
    /// cached, so its next use asks again.
    pub(crate) fn slot(&mut self, key: R::Key) -> Option<GlyphSlot> {
        let frame = self.frame;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = frame;
            return Some(entry.slot);
        }

        let image = self.rasterizer.rasterize(key)?;
        let color_page = image.content() == GlyphContent::Color;
        if image.width() == 0 || image.height() == 0 {
            let slot = GlyphSlot {
                texel: [0, 0],
                size: [0, 0],
                left: image.left(),
                top: image.top(),
                color_page,
            };
            self.entries.insert(
                key,
                Entry {
                    slot,
                    alloc: None,
                    last_used: frame,
                },
            );
            return Some(slot);
        }

        let allocation = self.allocate(color_page, &image)?;
        let slot = GlyphSlot {
            texel: [
                allocation.rectangle.min.x as u32,
                allocation.rectangle.min.y as u32,
            ],
            size: [image.width(), image.height()],
            left: image.left(),
            top: image.top(),
            color_page,
        };
        page_mut(&mut self.mask, &mut self.color, color_page).upload(
            &self.queue,
            slot.texel,
            slot.size,
            image.data(),
        );
        self.entries.insert(
            key,
            Entry {
                slot,
                alloc: Some(allocation.id),
                last_used: frame,
            },
        );
        Some(slot)
    }

    /// Finds room for `image` on its page: as is, then after evicting the
    /// slots this frame has not used, then after growing the page.
    fn allocate(&mut self, color_page: bool, image: &GlyphImage) -> Option<etagere::Allocation> {
        let max = self.device.limits().max_texture_dimension_2d;
        if image.width() > max || image.height() > max {
            if !self.reported_full {
                self.reported_full = true;
                tracing::warn!(
                    width = image.width(),
                    height = image.height(),
                    limit = max,
                    "glyph bitmap exceeds the device's texture limit; it is not placed"
                );
            }
            return None;
        }
        let wanted = size2(
            i32::try_from(image.width()).ok()?,
            i32::try_from(image.height()).ok()?,
        );
        let frame = self.frame;
        loop {
            let page = page_mut(&mut self.mask, &mut self.color, color_page);
            if let Some(allocation) = page.packer.allocate(wanted) {
                return Some(allocation);
            }
            if page.evict_unused(&mut self.entries, frame) > 0 {
                continue;
            }
            if page.grow(
                &mut self.entries,
                frame,
                &self.device,
                &self.queue,
                &mut self.rasterizer,
            ) {
                self.bind_group = create_bind_group(
                    &self.device,
                    &self.layout,
                    &self.mask,
                    &self.color,
                    &self.sampler,
                );
                continue;
            }
            if !self.reported_full {
                self.reported_full = true;
                let page = page_mut(&mut self.mask, &mut self.color, color_page);
                tracing::warn!(
                    page = page.label(),
                    size = page.size,
                    "glyph atlas page is full at the device's texture limit; glyphs are being dropped this frame"
                );
            }
            return None;
        }
    }

    /// Once per presented frame, after its submit: slots the next frame does
    /// not touch become reclaimable.
    pub(crate) fn end_frame(&mut self) {
        self.mask.release_retired();
        self.color.release_retired();
        self.frame = self.frame.wrapping_add(1);
        self.reported_full = false;
    }

    /// How many glyphs the atlas holds, both pages, empty glyphs included.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// The page a glyph of the given kind lives on, as a borrow disjoint from
/// the atlas' device/queue/rasterizer handles.
fn page_mut<'a>(mask: &'a mut Page, color: &'a mut Page, color_page: bool) -> &'a mut Page {
    if color_page { color } else { mask }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    mask: &Page,
    color: &Page,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Glyph Atlas Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&mask.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&color.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// The atlas over rasterizers other than the default: a scripted fake that
/// shows what the atlas does with each answer, and the Parley path's swash
/// rasterizer.
#[cfg(all(test, feature = "testing"))]
mod rasterizer_tests {
    use std::sync::Arc;

    use flui_painting::glyphs::{FaceKey, GlyphKey, SubpixelBin, SwashRasterizer};
    use flui_painting::{GlyphContent, GlyphImage, GlyphRasterizer};

    use super::GlyphAtlas;

    fn atlas<R: GlyphRasterizer>(rasterizer: R) -> GlyphAtlas<R> {
        let (device, queue) = crate::test_support::test_device_and_queue("Glyph Atlas Test");
        atlas_on(device, queue, rasterizer)
    }

    fn atlas_on<R: GlyphRasterizer>(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        rasterizer: R,
    ) -> GlyphAtlas<R> {
        let pipelines =
            crate::pipeline_set::PipelineSet::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        GlyphAtlas::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            &pipelines.glyph_atlas_bind_group_layout,
            rasterizer,
        )
    }

    const FACE: FaceKey = FaceKey {
        blob_id: 1,
        index: 0,
    };

    fn swash() -> SwashRasterizer {
        let mut rasterizer = SwashRasterizer::new();
        rasterizer
            .fonts_mut()
            .register_face(FACE, Arc::new(flui_painting::fonts::ROBOTO_REGULAR))
            .expect("Roboto is a face");
        rasterizer
    }

    fn key(glyph_id: u16, size: f32) -> GlyphKey {
        GlyphKey::new(FACE, glyph_id, size, SubpixelBin::Zero)
    }

    #[test]
    fn swash_glyphs_land_and_equal_keys_share_a_slot() {
        let mut atlas = atlas(swash());
        let first: Vec<_> = (1..=200)
            .map(|gid| atlas.slot(key(gid, 18.0)).expect("placed"))
            .collect();
        let held = atlas.len();
        for (gid, before) in (1..=200).zip(&first) {
            let again = atlas.slot(key(gid, 18.0)).expect("placed");
            assert_eq!(again.texel, before.texel, "glyph {gid}");
            assert_eq!(again.size, before.size, "glyph {gid}");
        }
        assert_eq!(atlas.len(), held, "the second pass hits the cache");
        assert!(first.iter().all(|slot| !slot.color_page), "Roboto is masks");
        assert!(
            first.iter().filter(|slot| !slot.is_empty()).count() > 100,
            "most glyphs have ink"
        );
    }

    // The public painter always owns SwashRasterizer. This private seam models
    // failed replay after the atlas has already admitted a valid bitmap.
    #[derive(Clone, Copy, Default)]
    enum ReplayChange {
        #[default]
        None,
        Size,
        Content,
    }

    #[derive(Default)]
    struct ReplayRasterizer {
        calls: [usize; 3],
        missing: bool,
        change: ReplayChange,
    }

    impl GlyphRasterizer for ReplayRasterizer {
        type Key = usize;

        fn rasterize(&mut self, key: usize) -> Option<GlyphImage> {
            self.calls[key] += 1;
            if self.missing && key == 0 && self.calls[key] == 2 {
                return None;
            }
            let change = if key == 1 && self.calls[key] == 2 {
                self.change
            } else {
                ReplayChange::None
            };
            let (width, content) = match change {
                ReplayChange::None => (32, GlyphContent::Mask),
                ReplayChange::Size => (33, GlyphContent::Mask),
                ReplayChange::Content => (32, GlyphContent::Color),
            };
            Some(
                GlyphImage::try_new(
                    0,
                    32,
                    width,
                    32,
                    content,
                    vec![255; (width * 32 * content.bytes_per_texel()) as usize],
                )
                .expect("scripted bitmap has valid bytes"),
            )
        }
    }

    fn replay_recovery(missing: bool, change: ReplayChange) {
        let mut atlas = atlas(ReplayRasterizer {
            missing,
            change,
            ..Default::default()
        });
        let before = [
            atlas.slot(0).expect("initial missing candidate"),
            atlas.slot(1).expect("initial mismatch candidate"),
            atlas.slot(2).expect("healthy glyph"),
        ];
        assert!(atlas.mask.grow(
            &mut atlas.entries,
            atlas.frame,
            &atlas.device,
            &atlas.queue,
            &mut atlas.rasterizer,
        ));

        let changed = !matches!(change, ReplayChange::None);
        for (key, failed) in [(0, missing), (1, changed), (2, false)] {
            let recovered = atlas.slot(key).expect("next use retries failed replay");
            assert_eq!(recovered.size, before[key].size);
            assert_eq!(
                atlas.rasterizer.calls[key],
                if failed { 3 } else { 2 },
                "failed replay must not remain a cache hit for key {key}"
            );
            if failed {
                // Every initial slot was used this frame. Its recorded quad
                // must never sample a different glyph admitted after the grow.
                for old in before {
                    assert!(
                        recovered.texel[0] + recovered.size[0] <= old.texel[0]
                            || old.texel[0] + old.size[0] <= recovered.texel[0]
                            || recovered.texel[1] + recovered.size[1] <= old.texel[1]
                            || old.texel[1] + old.size[1] <= recovered.texel[1],
                        "retry reused a recorded glyph region"
                    );
                }
            } else {
                assert_eq!(recovered.texel, before[key].texel);
            }
        }
        atlas.end_frame();
        for key in 0..3 {
            let calls = atlas.rasterizer.calls[key];
            assert!(
                atlas.slot(key).is_some(),
                "recovery survives frame retirement"
            );
            assert_eq!(atlas.rasterizer.calls[key], calls);
        }
    }

    fn missing_replay_retries() {
        replay_recovery(true, ReplayChange::None);
    }

    fn resized_replay_retries() {
        replay_recovery(false, ReplayChange::Size);
    }

    fn recolored_replay_retries() {
        replay_recovery(false, ReplayChange::Content);
    }

    fn missing_and_resized_replay_retry() {
        replay_recovery(true, ReplayChange::Size);
    }

    fn missing_and_recolored_replay_retry() {
        replay_recovery(true, ReplayChange::Content);
    }

    fn oversized_admission(extent: [u32; 2]) {
        struct OversizedRasterizer {
            extent: [u32; 2],
            calls: [usize; 2],
        }
        impl GlyphRasterizer for OversizedRasterizer {
            type Key = usize;

            fn rasterize(&mut self, key: usize) -> Option<GlyphImage> {
                self.calls[key] += 1;
                let [width, height] = if key == 0 && self.calls[key] == 1 {
                    self.extent
                } else {
                    [1, 1]
                };
                Some(
                    GlyphImage::try_new(
                        0,
                        1,
                        width,
                        height,
                        GlyphContent::Mask,
                        vec![255; (width * height) as usize],
                    )
                    .expect("device-oversized bitmap has valid CPU bytes"),
                )
            }
        }
        // Keep the device limit small so the source-negative control cannot
        // allocate large textures while demonstrating unnecessary growth.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            apply_limit_buckets: false,
            ..Default::default()
        }))
        .expect("GPU adapter for atlas admission");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_texture_dimension_2d: 512,
                ..Default::default()
            },
            ..Default::default()
        }))
        .expect("bounded atlas admission device");
        let mut atlas = atlas_on(
            Arc::new(device),
            Arc::new(queue),
            OversizedRasterizer {
                extent,
                calls: [0; 2],
            },
        );
        let healthy = atlas.slot(1).expect("healthy glyph before rejection");
        assert!(
            atlas.slot(0).is_none(),
            "oversized bitmap must not be admitted"
        );
        assert_eq!(
            atlas.rasterizer.calls[1], 1,
            "device-oversized admission must not grow the page and replay healthy glyphs"
        );
        assert_eq!(
            atlas
                .slot(1)
                .expect("healthy cache survives rejection")
                .texel,
            healthy.texel
        );
        assert_eq!(
            atlas.slot(0).expect("next valid bitmap retries").size,
            [1, 1]
        );
        atlas.end_frame();
        assert!(atlas.slot(0).is_some(), "retry survives frame retirement");
    }

    fn oversized_width_retries_without_growth() {
        oversized_admission([513, 1]);
    }

    fn oversized_height_retries_without_growth() {
        oversized_admission([1, 513]);
    }

    #[test]
    fn failed_glyph_replay_retries_without_reusing_recorded_regions() {
        let mut failed = Vec::new();
        for (name, case) in [
            ("missing bitmap", missing_replay_retries as fn()),
            ("changed dimensions", resized_replay_retries as fn()),
            ("changed content", recolored_replay_retries as fn()),
            (
                "missing and resized",
                missing_and_resized_replay_retry as fn(),
            ),
            (
                "missing and recolored",
                missing_and_recolored_replay_retry as fn(),
            ),
            (
                "device-oversized width",
                oversized_width_retries_without_growth as fn(),
            ),
            (
                "device-oversized height",
                oversized_height_retries_without_growth as fn(),
            ),
        ] {
            if std::panic::catch_unwind(case).is_err() {
                failed.push(name);
            }
        }
        assert!(
            failed.is_empty(),
            "glyph replay recovery failed: {failed:?}"
        );
    }
}
