//! The glyph atlas: rasterised glyph bitmaps packed into two GPU pages.
//!
//! The engine does not shape text and does not rasterise it either: a
//! paragraph arrives as an [`flui_painting::TextLayout`] the recorder shaped
//! against the shared font system (ADR-0065), and every glyph bitmap comes
//! from [`SharedFontSystem::rasterize`], keyed by the opaque
//! [`GlyphKey`] the layout places (ADR-0067). What the engine owns is the
//! cache: where each bitmap sits, how long it stays, and the bind group the
//! glyph pipeline samples it through.
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
use flui_painting::{GlyphContent, GlyphImage, GlyphKey, SharedFontSystem};
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
    fn evict_unused(&mut self, entries: &mut FxHashMap<GlyphKey, Entry>, frame: u64) -> usize {
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
    fn grow(
        &mut self,
        entries: &FxHashMap<GlyphKey, Entry>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: &SharedFontSystem,
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
        // rare and the scaler is deterministic for a key.
        for (key, entry) in entries {
            if entry.slot.color_page != self.is_color || entry.slot.is_empty() {
                continue;
            }
            let Some(image) = fonts.rasterize(*key) else {
                continue;
            };
            self.upload(queue, entry.slot.texel, entry.slot.size, &image.data);
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

/// The rasterised-glyph cache the glyph pipeline samples.
pub(crate) struct GlyphAtlas {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    /// The font system the paragraphs were shaped against; bitmaps come
    /// from it and it is never mutated here.
    fonts: SharedFontSystem,
    mask: Page,
    color: Page,
    /// Every glyph on either page, keyed for the record-time lookup.
    entries: FxHashMap<GlyphKey, Entry>,
    sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    /// The current frame; slots used this frame are immune to eviction.
    frame: u64,
    /// Whether a glyph was dropped this frame because a page could not grow.
    reported_full: bool,
}

impl GlyphAtlas {
    pub(crate) fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        layout: &wgpu::BindGroupLayout,
        fonts: SharedFontSystem,
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
            fonts,
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
    /// `None` when the glyph cannot be placed: the page is at the device
    /// limit and every slot is in use this frame. The glyph is skipped and
    /// the condition reported once per frame.
    pub(crate) fn slot(&mut self, key: GlyphKey) -> Option<GlyphSlot> {
        let frame = self.frame;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = frame;
            return Some(entry.slot);
        }

        let image = self.fonts.rasterize(key)?;
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
            if page.grow(&self.entries, &self.device, &self.queue, &self.fonts) {
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
}

/// The page a glyph of the given kind lives on, as a borrow disjoint from
/// the atlas' device/queue/fonts handles.
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
