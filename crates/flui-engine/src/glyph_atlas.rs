//! The glyph atlas: rasterised glyph bitmaps packed into two GPU pages.
//!
//! The engine does not shape text and does not rasterise it either: a
//! paragraph arrives as an [`flui_painting::TextLayout`] the recorder shaped
//! against the shared font system (ADR-0065), and every glyph bitmap comes
//! from the atlas's [`GlyphRasterizer`], keyed by the opaque key the layout
//! places (ADR-0067). By default that is the cosmic-text font system,
//! [`SharedFontSystem`], keyed by [`flui_painting::GlyphKey`]; ADR-0092 §10
//! moves it to the Parley path's rasterizer. What the engine owns is the
//! cache: where each bitmap sits, how long it stays, and the bind group the
//! glyph pipeline samples it through.
//!
//! A rasterizer is trusted to be deterministic, but not to the point of a
//! GPU validation panic: an image whose data does not match its size is not
//! placed, and a grow re-uploads only images that still fit the slot the
//! first one sized.
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
use flui_painting::{GlyphContent, GlyphImage, GlyphRasterizer, SharedFontSystem};
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
    /// A re-rasterized image that no longer fits its slot (another size,
    /// another content kind, or data of the wrong length) is not uploaded:
    /// its slot is left blank in the new texture rather than failing the
    /// upload. Warned once per grow.
    fn grow<R: GlyphRasterizer>(
        &mut self,
        entries: &FxHashMap<R::Key, Entry>,
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
        let mut mismatched = None;
        for (key, entry) in entries {
            if entry.slot.color_page != self.is_color || entry.slot.is_empty() {
                continue;
            }
            let Some(image) = rasterizer.rasterize(*key) else {
                continue;
            };
            if [image.width, image.height] != entry.slot.size
                || image.content != content
                || !fits(&image)
            {
                mismatched.get_or_insert(*key);
                continue;
            }
            self.upload(queue, entry.slot.texel, entry.slot.size, &image.data);
        }
        if let Some(key) = mismatched {
            tracing::warn!(
                ?key,
                page = self.label(),
                "a glyph re-rasterized to another bitmap on atlas grow; its slot is left blank"
            );
        }
        true
    }
}

/// Whether `image.data` holds exactly the texels its size and content name,
/// so an upload of it cannot fail wgpu's copy validation.
fn fits(image: &GlyphImage) -> bool {
    let texels = u64::from(image.width) * u64::from(image.height);
    u64::try_from(image.data.len())
        .is_ok_and(|len| len == texels * u64::from(image.content.bytes_per_texel()))
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

/// The rasterised-glyph cache the glyph pipeline samples.
///
/// Generic over where bitmaps come from; the default is the cosmic-text font
/// system the paragraphs were shaped against.
pub(crate) struct GlyphAtlas<R: GlyphRasterizer = SharedFontSystem> {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    /// Where bitmaps come from. Owned by the atlas and taken by `&mut`; the
    /// default's font database is never mutated here.
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
    /// key, or returned data that does not match the image's size (warned),
    /// or the page is at the device limit and every slot is in use this
    /// frame (reported once per frame). The glyph is skipped and nothing is
    /// cached, so its next use asks again.
    pub(crate) fn slot(&mut self, key: R::Key) -> Option<GlyphSlot> {
        let frame = self.frame;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = frame;
            return Some(entry.slot);
        }

        let image = self.rasterizer.rasterize(key)?;
        if !fits(&image) {
            tracing::warn!(
                ?key,
                width = image.width,
                height = image.height,
                bytes = image.data.len(),
                "a rasterized glyph's data does not match its size; it is not placed"
            );
            return None;
        }
        let color_page = image.content == GlyphContent::Color;
        if image.width == 0 || image.height == 0 {
            let slot = GlyphSlot {
                texel: [0, 0],
                size: [0, 0],
                left: image.left,
                top: image.top,
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
            size: [image.width, image.height],
            left: image.left,
            top: image.top,
            color_page,
        };
        page_mut(&mut self.mask, &mut self.color, color_page).upload(
            &self.queue,
            slot.texel,
            slot.size,
            &image.data,
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
        let wanted = size2(image.width as i32, image.height as i32);
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
                &self.entries,
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
        self.frame = self.frame.wrapping_add(1);
        self.reported_full = false;
    }

    /// How many glyphs the atlas holds, both pages, empty glyphs included.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The mask page's side in texels.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn mask_page_size(&self) -> u32 {
        self.mask.size
    }

    /// The colour page's side in texels.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn color_page_size(&self) -> u32 {
        self.color.size
    }

    /// The rasterizer, to register faces after the atlas exists.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn rasterizer_mut(&mut self) -> &mut R {
        &mut self.rasterizer
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

#[cfg(all(test, feature = "testing"))]
mod tests {
    use std::sync::Arc;

    use flui_painting::TextLayout;
    use flui_types::typography::TextDirection;

    use super::GlyphAtlas;

    fn atlas() -> GlyphAtlas {
        let (device, queue) = crate::test_support::test_device_and_queue("Glyph Atlas Test");
        let pipelines =
            crate::pipeline_set::PipelineSet::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        GlyphAtlas::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            &pipelines.glyph_atlas_bind_group_layout,
            flui_painting::shared_font_system(),
        )
    }

    fn keys(text: &str, size: f32) -> Vec<flui_painting::GlyphKey> {
        TextLayout::new(text, None, size, None, None, TextDirection::Ltr)
            .placed_glyphs((0.0, 0.0), 1.0)
            .map(|g| g.key)
            .collect()
    }

    /// A glyph drawn twice occupies one slot; a glyph with no ink (a space)
    /// is remembered without occupying atlas space.
    ///
    /// The same string is shaped twice: two `a`s in one paragraph sit at
    /// different subpixel offsets and so are different keys by design.
    #[test]
    fn a_slot_is_shared_by_equal_keys_and_an_empty_glyph_takes_no_space() {
        let mut atlas = atlas();
        let first_run = keys("a b", 16.0);
        let second_run = keys("a b", 16.0);
        assert_eq!(first_run.len(), 3);
        let first = atlas.slot(first_run[0]).expect("'a' rasterises");
        let space = atlas.slot(first_run[1]).expect("a space is a glyph");
        let again = atlas.slot(second_run[0]).expect("'a' again");
        assert_eq!(first.texel, again.texel, "equal keys share one slot");
        assert!(space.is_empty(), "a space has no texels");
        assert_eq!(atlas.len(), 2, "'a' once, space once");
    }

    /// Slots unused for a frame are reclaimed before a page grows: churning
    /// through more distinct glyphs than a page holds leaves the page at its
    /// initial size, while glyphs used in the SAME frame are never evicted.
    #[test]
    fn eviction_reclaims_slots_before_the_page_grows() {
        let mut atlas = atlas();
        let initial = atlas.mask_page_size();
        // 96 distinct sizes × ~26 glyphs at up to 40px: far more texels than
        // a 256² page, across frames that each use only one size.
        for frame in 0..96u32 {
            let size = 12.0 + frame as f32 * 0.3;
            for key in keys("abcdefghijklmnopqrstuvwxyz", size) {
                assert!(atlas.slot(key).is_some(), "frame {frame}: glyph placed");
            }
            atlas.end_frame();
        }
        assert_eq!(
            atlas.mask_page_size(),
            initial,
            "a page that can evict never needs to grow"
        );
    }

    /// When one frame needs more than a page holds, the page grows and every
    /// glyph placed earlier in that frame keeps its texel position.
    #[test]
    fn a_page_grows_within_a_frame_and_earlier_slots_keep_their_place() {
        let mut atlas = atlas();
        let initial = atlas.mask_page_size();
        let early = keys("FLUI", 40.0);
        let early_slots: Vec<_> = early.iter().map(|k| atlas.slot(*k).unwrap()).collect();
        for step in 0..60u32 {
            for key in keys("abcdefghijklmnopqrstuvwxyz", 20.0 + step as f32 * 0.5) {
                atlas.slot(key);
            }
        }
        assert!(
            atlas.mask_page_size() > initial,
            "one frame's glyphs exceed a {initial}² page, so it must grow"
        );
        for (key, before) in early.iter().zip(early_slots) {
            let after = atlas.slot(*key).unwrap();
            assert_eq!(
                after.texel, before.texel,
                "a grow keeps allocations in place"
            );
            assert_eq!(after.size, before.size);
        }
    }
}

/// The atlas over rasterizers other than the default: a scripted fake that
/// shows what the atlas does with each answer, and the Parley path's swash
/// rasterizer.
#[cfg(all(test, feature = "testing"))]
mod rasterizer_tests {
    use std::sync::Arc;

    use flui_painting::parley_text::{FaceKey, ParleyGlyphKey, SubpixelBin, SwashRasterizer};
    use flui_painting::{GlyphContent, GlyphImage, GlyphRasterizer};
    use rustc_hash::FxHashMap;

    use super::GlyphAtlas;

    /// Answers from a table, counts every call, and optionally answers every
    /// call after a key's first with `second_call` instead.
    #[derive(Default)]
    struct FakeRasterizer {
        images: FxHashMap<u32, GlyphImage>,
        calls: FxHashMap<u32, usize>,
        second_call: Option<GlyphImage>,
    }

    impl GlyphRasterizer for FakeRasterizer {
        type Key = u32;

        fn rasterize(&mut self, key: u32) -> Option<GlyphImage> {
            let calls = self.calls.entry(key).or_default();
            *calls += 1;
            if *calls > 1
                && let Some(image) = &self.second_call
            {
                return Some(image.clone());
            }
            self.images.get(&key).cloned()
        }
    }

    fn image(width: u32, height: u32, content: GlyphContent) -> GlyphImage {
        let len = (width * height * content.bytes_per_texel()) as usize;
        GlyphImage {
            left: 0,
            top: i32::try_from(height).expect("small"),
            width,
            height,
            content,
            data: vec![0xff; len],
        }
    }

    fn atlas<R: GlyphRasterizer>(rasterizer: R) -> GlyphAtlas<R> {
        let (device, queue) = crate::test_support::test_device_and_queue("Glyph Atlas Test");
        let pipelines =
            crate::pipeline_set::PipelineSet::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        GlyphAtlas::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            &pipelines.glyph_atlas_bind_group_layout,
            rasterizer,
        )
    }

    /// Forty 48² masks, more than a 256² page holds: placed in one frame, the
    /// page must grow once.
    fn crowded() -> FakeRasterizer {
        let mut fake = FakeRasterizer::default();
        for key in 0..40 {
            fake.images.insert(key, image(48, 48, GlyphContent::Mask));
        }
        fake
    }

    /// Places every key of [`crowded`] in one frame. Returns the keys placed
    /// before the mask page grew.
    fn place_crowded(atlas: &mut GlyphAtlas<FakeRasterizer>) -> Vec<u32> {
        let initial = atlas.mask_page_size();
        let mut before_grow = Vec::new();
        for key in 0..40 {
            let slot = atlas.slot(key).expect("a grown page has room");
            assert_eq!(slot.size, [48, 48]);
            if atlas.mask_page_size() == initial {
                before_grow.push(key);
            }
        }
        assert!(atlas.mask_page_size() > initial, "the page grew");
        before_grow
    }

    #[test]
    fn colour_images_land_on_the_colour_page() {
        let mut fake = FakeRasterizer::default();
        fake.images.insert(1, image(4, 4, GlyphContent::Color));
        fake.images.insert(2, image(4, 4, GlyphContent::Mask));
        let mut atlas = atlas(fake);
        assert!(atlas.slot(1).expect("placed").color_page);
        assert!(!atlas.slot(2).expect("placed").color_page);
        assert_eq!(atlas.len(), 2);
    }

    #[test]
    fn a_key_the_rasterizer_cannot_place_is_not_cached() {
        let mut atlas = atlas(FakeRasterizer::default());
        assert!(atlas.slot(9).is_none());
        assert_eq!(atlas.len(), 0);
        assert!(atlas.slot(9).is_none());
        assert_eq!(
            atlas.rasterizer_mut().calls[&9],
            2,
            "the next use asks again"
        );
    }

    #[test]
    fn an_image_whose_data_does_not_match_its_size_is_not_placed() {
        let mut fake = FakeRasterizer::default();
        let mut short = image(4, 4, GlyphContent::Mask);
        short.data.truncate(10);
        fake.images.insert(1, short);
        let mut one_byte_colour = image(4, 4, GlyphContent::Color);
        one_byte_colour.data.truncate(16);
        fake.images.insert(2, one_byte_colour);
        let mut atlas = atlas(fake);
        assert!(atlas.slot(1).is_none(), "a mask missing texels");
        assert!(
            atlas.slot(2).is_none(),
            "a colour image with one byte per texel"
        );
        assert_eq!(atlas.len(), 0);
    }

    /// A grow re-uploads each live glyph from one more rasterization, and
    /// never rasterizes a glyph it is not holding.
    #[test]
    fn a_grow_rerasterizes_each_live_glyph_once() {
        let mut atlas = atlas(crowded());
        let before_grow = place_crowded(&mut atlas);
        assert!(!before_grow.is_empty());
        let calls = &atlas.rasterizer_mut().calls;
        for key in 0..40 {
            let expected = if before_grow.contains(&key) { 2 } else { 1 };
            assert_eq!(calls[&key], expected, "key {key}");
        }
    }

    /// A rasterizer that answers a grow with a smaller bitmap would fail
    /// wgpu's copy validation (the default error handler panics); the atlas
    /// skips the upload and the slot keeps its size.
    #[test]
    fn a_bitmap_that_changes_on_grow_is_not_uploaded() {
        let mut fake = crowded();
        fake.second_call = Some(image(8, 8, GlyphContent::Mask));
        let mut atlas = atlas(fake);
        let before_grow = place_crowded(&mut atlas);
        assert!(!before_grow.is_empty());
        for key in before_grow {
            assert_eq!(atlas.slot(key).expect("still placed").size, [48, 48]);
        }
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

    fn key(glyph_id: u16, size: f32) -> ParleyGlyphKey {
        ParleyGlyphKey::new(FACE, glyph_id, size, SubpixelBin::Zero)
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

    #[test]
    fn swash_keys_keep_their_slot_through_a_grow() {
        let mut atlas = atlas(swash());
        let initial = atlas.mask_page_size();
        let early: Vec<_> = (1..=20)
            .map(|gid| (gid, atlas.slot(key(gid, 40.0)).expect("placed")))
            .collect();
        for gid in 21..=400 {
            atlas.slot(key(gid, 40.0));
        }
        assert!(atlas.mask_page_size() > initial, "the page grew");
        assert_eq!(atlas.color_page_size(), initial, "the colour page did not");
        for (gid, before) in early {
            let after = atlas.slot(key(gid, 40.0)).expect("placed");
            assert_eq!(after.texel, before.texel, "glyph {gid}");
            assert_eq!(after.size, before.size, "glyph {gid}");
        }
    }

    #[test]
    fn a_face_registered_after_the_atlas_exists_is_placed() {
        let mut atlas = atlas(SwashRasterizer::new());
        let a = key(68, 18.0);
        assert!(atlas.slot(a).is_none(), "no face yet");
        atlas
            .rasterizer_mut()
            .fonts_mut()
            .register_face(FACE, Arc::new(flui_painting::fonts::ROBOTO_REGULAR))
            .expect("Roboto is a face");
        let slot = atlas.slot(a).expect("the face is registered now");
        assert!(!slot.is_empty());
    }
}
