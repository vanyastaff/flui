//! One GPU context serves every presentation as a slot (ADR-0305 §1).
//!
//! [`GpuContext`] owns the instance, the adapter, the device and its queue,
//! and one painter, whose pipeline library (one set per target format), glyph
//! atlas, image and path caches every slot shares. What is keyed by the
//! presentation is the slot's: its target texture, its size and its format.
//!
//! Slots render one at a time on the context's one mutator, so sharing the
//! caches takes no lock: the context is `Send`, so a raster thread can own it,
//! and `!Sync`, so no `&GpuContext` crosses a thread.
//!
//! Today the context serves offscreen slots: the headless renderer is made of
//! one, and the raster executor drives it as its backend. Window surfaces are
//! the step ADR-0305's Migration names next.

use std::{cell::Cell, marker::PhantomData, sync::Arc};

use flui_foundation::{geometry::DeviceSize, memory::MemoryPressureLevel};
use flui_layer::{DamageRegion, LayerTree, Scene};

use crate::{
    device_domain::DeviceDomain,
    error::{EngineError, EngineResult},
    layer_dispatcher::LayerDispatcher,
    painter::WgpuPainter,
    raster::PresentDisposition,
    raster_executor::{SlotBackend, SlotId, SlotTarget},
};

#[cfg(test)]
thread_local! {
    /// `wgpu::Instance`s this thread's contexts created. `wgpu::Adapter`'s
    /// equality cannot tell whether two contexts share an instance (an
    /// adapter from a fresh instance compares equal), so tests count instead.
    /// Per thread, so tests running in parallel threads cannot move each
    /// other's count.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    pub(crate) static INSTANCES_CREATED: Cell<usize> = const { Cell::new(0) };
}

/// What one slot renders into.
struct SurfaceSlot {
    /// The slot's target, which the executor reads back.
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    size: (u32, u32),
}

/// The slots of one context, by the index of the id the owner minted.
#[derive(Default)]
struct SlotTable {
    entries: Vec<Option<(SlotId, SurfaceSlot)>>,
}

impl SlotTable {
    /// Hold `slot` under `id`; `false` when `id`'s index is taken.
    fn insert(&mut self, id: SlotId, slot: SurfaceSlot) -> bool {
        let index = id.index() as usize;
        if self.entries.len() <= index {
            self.entries.resize_with(index + 1, || None);
        }
        if self.entries[index].is_some() {
            return false;
        }
        self.entries[index] = Some((id, slot));
        true
    }

    fn get_mut(&mut self, id: SlotId) -> Option<&mut SurfaceSlot> {
        match self.entries.get_mut(id.index() as usize)? {
            Some((held, slot)) if *held == id => Some(slot),
            _ => None,
        }
    }

    fn get(&self, id: SlotId) -> Option<&SurfaceSlot> {
        match self.entries.get(id.index() as usize)? {
            Some((held, slot)) if *held == id => Some(slot),
            _ => None,
        }
    }

    /// Takes the slot out; `None` for a stale id or one already detached.
    fn remove(&mut self, id: SlotId) -> Option<SurfaceSlot> {
        let entry = self.entries.get_mut(id.index() as usize)?;
        if entry.as_ref().is_some_and(|(held, _)| *held == id) {
            return entry.take().map(|(_, slot)| slot);
        }
        None
    }
}

/// What a context has built over its life, read by the tests that pin the
/// sharing ADR-0305 §1 decides.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextBuilds {
    /// Devices requested: one per context.
    pub(crate) devices: usize,
    /// Glyph atlases built: one per painter the context built.
    pub(crate) glyph_atlases: usize,
    /// Pipeline sets built: one per painter and per further target format.
    pub(crate) pipeline_sets: usize,
}

/// The one GPU context of an application (ADR-0305 §1).
///
/// Owned by the code that rasterises; never a `static` and never reached
/// ambiently (ADR-0097). `Send`, so a raster thread can take it, and not
/// `Sync`, so it has one mutator and its caches need no lock.
pub struct GpuContext {
    /// Kept for the device's life; the surfaces of later windows are made
    /// on it (ADR-0305 §2).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "held for the device's life; window slots create surfaces on it"
        )
    )]
    instance: wgpu::Instance,
    /// Kept so a test's feature-reduced twin comes from this adapter and
    /// instance (`HeadlessRenderer::without_dual_source_blending`).
    #[cfg_attr(not(test), expect(dead_code, reason = "held for the device's life"))]
    adapter: wgpu::Adapter,
    domain: Arc<DeviceDomain>,
    /// The painter every slot records and replays with: its pipelines per
    /// target format, the glyph atlas and the image and path caches.
    ///
    /// Taken for the length of a frame, so a frame that fails or unwinds
    /// discards it and the next frame builds a fresh one; only a complete
    /// frame puts it back.
    painter: Option<WgpuPainter>,
    slots: SlotTable,
    #[cfg(test)]
    builds: ContextBuilds,
    /// When set, the next frame panics after taking the painter.
    #[cfg(test)]
    panic_mid_frame: bool,
    _one_mutator: PhantomData<Cell<()>>,
}

static_assertions::assert_impl_all!(GpuContext: Send);
static_assertions::assert_not_impl_any!(GpuContext: Sync);

impl GpuContext {
    /// A context with no window: offscreen slots only (headless, tests).
    ///
    /// Requests whichever of `wanted_features` the adapter offers.
    /// `WGPU_BACKEND` picks the backend, so a readback suite runs on the GL
    /// fallback with `WGPU_BACKEND=gl` (ADR-0244 §1).
    ///
    /// # Errors
    /// [`EngineError`] when no GPU adapter or device is available.
    pub(crate) async fn offscreen(wanted_features: wgpu::Features) -> EngineResult<Self> {
        #[cfg(test)]
        INSTANCES_CREATED.with(|created| created.set(created.get() + 1));
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
        let adapter = instance
            .request_adapter(&crate::adapter::trusted_adapter_options(
                wgpu::PowerPreference::HighPerformance,
                None,
            ))
            .await
            .map_err(EngineError::adapter_request)?;
        Self::on_adapter(instance, adapter, wanted_features).await
    }

    /// A second context on this one's adapter and instance, its device
    /// requested with `wanted_features`.
    #[cfg(test)]
    pub(crate) fn twin(
        &self,
        wanted_features: wgpu::Features,
    ) -> impl std::future::Future<Output = EngineResult<Self>> + use<> {
        Self::on_adapter(self.instance.clone(), self.adapter.clone(), wanted_features)
    }

    async fn on_adapter(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        wanted_features: wgpu::Features,
    ) -> EngineResult<Self> {
        // Deliberately NOT `adapter::request_flui_device`: an offscreen
        // context wants wgpu's default (downlevel-friendly) device rather
        // than the renderer's capability-negotiated one, plus the features
        // asked for, which change what the rendered pixels look like.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("FLUI GPU Context Device"),
                required_features: adapter.features() & wanted_features,
                ..Default::default()
            })
            .await
            .map_err(EngineError::device_creation)?;
        let domain = DeviceDomain::new(Arc::new(device), Arc::new(queue));
        Ok(Self {
            instance,
            adapter,
            domain,
            painter: None,
            slots: SlotTable::default(),
            #[cfg(test)]
            builds: ContextBuilds {
                devices: 1,
                glyph_atlases: 0,
                pipeline_sets: 0,
            },
            #[cfg(test)]
            panic_mid_frame: false,
            _one_mutator: PhantomData,
        })
    }

    /// The device domain every slot submits through.
    #[cfg(test)]
    pub(crate) fn domain(&self) -> &Arc<DeviceDomain> {
        &self.domain
    }

    /// The device's features.
    #[cfg(test)]
    pub(crate) fn features(&self) -> wgpu::Features {
        self.domain.device().features()
    }

    /// The device's largest 2D texture side, the largest slot it renders.
    pub(crate) fn max_texture_dimension(&self) -> u32 {
        self.domain.device().limits().max_texture_dimension_2d
    }

    /// Attach an offscreen slot under `slot`, of `size` device pixels in
    /// `format`, an encoded RGBA8 or BGRA8 colour format.
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] for a zero or over-limit side;
    /// [`EngineError::InvalidRenderTarget`] for another format or an id
    /// whose index holds a slot.
    pub(crate) fn attach_offscreen(
        &mut self,
        slot: SlotId,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> EngineResult<()> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(EngineError::InvalidRenderTarget {
                reason: "an offscreen slot is RGBA8 or BGRA8",
            });
        }
        let texture = self.slot_texture(size, format)?;
        if self.slots.insert(
            slot,
            SurfaceSlot {
                texture,
                format,
                size,
            },
        ) {
            Ok(())
        } else {
            Err(EngineError::InvalidRenderTarget {
                reason: "the slot's index holds another slot",
            })
        }
    }

    /// Give `slot` a target of `size` device pixels; its previous contents
    /// are dropped. Nothing changes for a size the slot already has.
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] for a zero or over-limit side,
    /// which leaves the slot as it was; [`EngineError::InvalidRenderTarget`]
    /// for a slot that is not attached.
    pub(crate) fn resize_slot(&mut self, slot: SlotId, size: (u32, u32)) -> EngineResult<()> {
        let format = self.slot(slot)?.format;
        if self.slot(slot)?.size == size {
            return Ok(());
        }
        let texture = self.slot_texture(size, format)?;
        let entry = self.slot_mut(slot)?;
        entry.texture = texture;
        entry.size = size;
        Ok(())
    }

    /// Detach `slot`; `false` for a stale id or one already detached.
    pub(crate) fn detach(&mut self, slot: SlotId) -> bool {
        self.slots.remove(slot).is_some()
    }

    /// Clear `slot` to the frame background and draw `tree` over it.
    ///
    /// # Errors
    /// [`EngineError::InvalidRenderTarget`] for a slot that is not attached,
    /// or the frame's own render and submission errors.
    pub(crate) fn render_slot(&mut self, slot: SlotId, tree: &LayerTree) -> EngineResult<()> {
        let (texture, format, size) = {
            let entry = self.slot(slot)?;
            (entry.texture.clone(), entry.format, entry.size)
        };
        let _submission_scope = self.domain.begin_frame_scope()?;
        let mut painter = self.take_painter(format, size);
        #[cfg(test)]
        assert!(
            !std::mem::take(&mut self.panic_mid_frame),
            "injected panic in the middle of a slot's frame"
        );
        painter.begin_frame_in_scope();
        painter.resize(size.0, size.1);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        crate::headless::clear_to_background(&self.domain, &view)?;
        {
            let mut backend = LayerDispatcher::new(&mut painter);
            crate::layer_walk::record_layer_tree(tree, tree.root(), &mut backend)?;
        }
        let mut encoder =
            self.domain
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("FLUI Slot Render Encoder"),
                });
        painter.render_to_texture(&texture, &mut encoder)?;
        painter.submit_encoder(encoder)?;
        painter.finish_frame();
        self.painter = Some(painter);
        Ok(())
    }

    /// The pixels `slot` holds, tightly packed, top row first, four bytes a
    /// pixel in the slot format's channel order.
    ///
    /// # Errors
    /// [`EngineError::InvalidRenderTarget`] for a slot that is not attached;
    /// a readback error.
    pub(crate) fn read_slot(&self, slot: SlotId) -> EngineResult<Vec<u8>> {
        let entry = self.slot(slot)?;
        crate::headless::readback_rgba(&self.domain, &entry.texture, entry.size.0, entry.size.1)
    }

    /// What this context has built over its life.
    #[cfg(test)]
    pub(crate) fn builds(&self) -> ContextBuilds {
        self.builds
    }

    /// Make the next frame panic after it took the painter.
    #[cfg(test)]
    pub(crate) fn panic_in_next_frame(&mut self) {
        self.panic_mid_frame = true;
    }

    /// The shared painter, retargeted to `format` and sized for `size`; built
    /// first when no complete frame left one.
    fn take_painter(&mut self, format: wgpu::TextureFormat, size: (u32, u32)) -> WgpuPainter {
        let Some(mut painter) = self.painter.take() else {
            #[cfg(test)]
            {
                self.builds.glyph_atlases += 1;
                self.builds.pipeline_sets += 1;
            }
            return WgpuPainter::with_domain(Arc::clone(&self.domain), format, size);
        };
        #[cfg(test)]
        let sets_before = painter.pipeline_sets_built();
        painter.retarget(format);
        #[cfg(test)]
        {
            self.builds.pipeline_sets += painter.pipeline_sets_built() - sets_before;
        }
        painter
    }

    fn slot(&self, slot: SlotId) -> EngineResult<&SurfaceSlot> {
        self.slots
            .get(slot)
            .ok_or(EngineError::InvalidRenderTarget {
                reason: "the slot is not attached to this context",
            })
    }

    fn slot_mut(&mut self, slot: SlotId) -> EngineResult<&mut SurfaceSlot> {
        self.slots
            .get_mut(slot)
            .ok_or(EngineError::InvalidRenderTarget {
                reason: "the slot is not attached to this context",
            })
    }

    /// A slot target of `size` in `format`, bounded by the device's limit
    /// here, where the size enters: `create_texture` reports an over-limit
    /// size through wgpu's uncaptured-error path, which panics by default,
    /// and wgpu rejects a zero-sized texture and readback buffer by panicking.
    fn slot_texture(
        &self,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> EngineResult<wgpu::Texture> {
        let (width, height) = size;
        let max_dim = self.max_texture_dimension();
        if width == 0 || height == 0 || width > max_dim || height > max_dim {
            return Err(EngineError::InvalidTargetSize { width, height });
        }
        Ok(self
            .domain
            .device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("FLUI Offscreen Slot Target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }))
    }
}

/// The context as the raster executor's backend: its offscreen slots render
/// in full each frame and count as presented once submitted.
impl SlotBackend for GpuContext {
    fn attach(&mut self, slot: SlotId, target: SlotTarget) -> EngineResult<()> {
        match target {
            SlotTarget::Offscreen { size, format } => {
                self.attach_offscreen(slot, device_extent(size)?, format)
            }
        }
    }

    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> EngineResult<()> {
        self.resize_slot(slot, device_extent(size)?)
    }

    fn render(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        _damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        self.render_slot(slot, scene.tree())?;
        Ok(PresentDisposition::Presented)
    }

    fn detach(&mut self, slot: SlotId) {
        GpuContext::detach(self, slot);
    }

    fn reclaim(&mut self, level: MemoryPressureLevel) {
        if let Some(painter) = self.painter.as_mut() {
            painter.reclaim(level);
        }
    }
}

/// `size` as a texture extent; a side of zero or less is refused.
fn device_extent(size: DeviceSize) -> EngineResult<(u32, u32)> {
    match (u32::try_from(size.width), u32::try_from(size.height)) {
        (Ok(width), Ok(height)) if width > 0 && height > 0 => Ok((width, height)),
        _ => Err(EngineError::InvalidTargetSize {
            width: u32::try_from(size.width).unwrap_or(0),
            height: u32::try_from(size.height).unwrap_or(0),
        }),
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::{
        FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, PresentationId,
        SurfaceGeneration, UiRuntimeId,
        geometry::{DeviceSize, Offset, Rect},
    };
    use flui_layer::{DamageRegion, LayerTree, Scene, SceneBuilder, SceneSnapshot};
    use flui_painting::{
        Canvas, TextPainter,
        paint::Paint,
        styling::Color,
        typography::{TextDirection, TextSpan, TextStyle},
    };

    use super::{GpuContext, INSTANCES_CREATED};
    use crate::error::EngineError;
    use crate::raster_executor::{
        RasterCommand, RasterExecutor, RasterVerdict, SendReceipt, SlotConfig, SlotId, SlotTarget,
    };

    /// A red square beside a black glyph, so a frame samples both the shape
    /// pipelines and the glyph atlas.
    fn square_and_glyph() -> LayerTree {
        let mut painter = TextPainter::new()
            .with_text(
                TextSpan::new("I").with_style(
                    TextStyle::new()
                        .with_font_size(14.0)
                        .with_color(Color::rgb(0, 0, 0)),
                ),
            )
            .with_text_direction(TextDirection::Ltr);
        painter
            .layout(
                &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
                0.0,
                64.0,
            )
            .expect("the fixture's layout inputs are valid");
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
        );
        painter.paint(&mut canvas, Offset::new(10.0, 0.0));
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        builder.build()
    }

    fn context_or_skip() -> Option<GpuContext> {
        match pollster::block_on(GpuContext::offscreen(wgpu::Features::DUAL_SOURCE_BLENDING)) {
            Ok(context) => Some(context),
            Err(error) => {
                crate::test_support::resolve_unavailable_gpu(
                    &error.to_string(),
                    crate::test_support::require_gpu(),
                );
                None
            }
        }
    }

    /// A frame of `tree` for the slot at `address`, stamped with `generation`.
    fn frame(
        slot: SlotId,
        address: PresentationAddress,
        generation: SurfaceGeneration,
        tree: LayerTree,
    ) -> RasterCommand {
        RasterCommand::Frame {
            slot,
            snapshot: SceneSnapshot::new(
                FrameStamp::new(
                    address,
                    FrameEpoch::ZERO,
                    generation,
                    GpuResourceGeneration::ZERO,
                ),
                DamageRegion::Full,
                Scene::new(tree),
            ),
        }
    }

    /// Gate G5: slots of two formats and two sizes, driven through the raster
    /// executor, share one instance, one device, one glyph atlas and one
    /// pipeline set per format, and drawing every slot again builds nothing.
    /// Before the context, each headless capture and each window built all of
    /// them for itself.
    ///
    /// Red-check: build a painter per slot instead of sharing the context's
    /// (or rebuild it when the format changes) and the counts grow.
    #[test]
    fn every_slot_shares_one_device_one_glyph_atlas_and_one_pipeline_per_format() {
        let instances = || INSTANCES_CREATED.with(std::cell::Cell::get);
        let before = instances();
        let Some(context) = context_or_skip() else {
            return;
        };
        let (mut executor, sender) = RasterExecutor::new(context, || {});
        let targets = [
            ((32, 16), wgpu::TextureFormat::Rgba8Unorm),
            ((48, 16), wgpu::TextureFormat::Rgba8Unorm),
            ((32, 16), wgpu::TextureFormat::Bgra8Unorm),
        ];
        let slots = targets.map(|((width, height), format)| {
            let slot = sender.mint_slot().expect("a slot id is left");
            let address = PresentationAddress {
                ui_runtime_id: UiRuntimeId::new(1),
                presentation_id: PresentationId::new(slot.index() as usize + 1),
            };
            let receipt = sender
                .send(RasterCommand::Attach {
                    slot,
                    address,
                    target: SlotTarget::Offscreen {
                        size: DeviceSize::new(width, height),
                        format,
                    },
                    config: SlotConfig::new(),
                })
                .expect("the attach is accepted");
            let SendReceipt::Configured(generation) = receipt else {
                panic!("an attach of a positive size configures the slot: {receipt:?}");
            };
            (slot, address, generation)
        });
        let draw_all = |executor: &mut RasterExecutor<GpuContext>| {
            for (slot, address, generation) in slots {
                sender
                    .send(frame(slot, address, generation, square_and_glyph()))
                    .expect("the frame is accepted");
            }
            let _ = executor.turn();
            let mut settled = Vec::new();
            for (slot, ..) in slots {
                sender.drain_settlements(slot, &mut settled);
            }
            assert!(
                settled
                    .iter()
                    .all(|settlement| settlement.verdict == RasterVerdict::Presented),
                "every slot presents: {settled:?}"
            );
            assert_eq!(settled.len(), slots.len());
        };
        draw_all(&mut executor);
        let first = executor.backend().builds();
        assert_eq!(instances() - before, 1, "one instance serves every slot");
        assert_eq!(first.devices, 1, "one device serves every slot");
        assert_eq!(first.glyph_atlases, 1, "one glyph atlas serves every slot");
        assert_eq!(
            first.pipeline_sets, 2,
            "one pipeline set per target format, two formats"
        );

        draw_all(&mut executor);
        draw_all(&mut executor);
        assert_eq!(
            executor.backend().builds(),
            first,
            "drawing every slot again builds no device, atlas or pipeline"
        );

        // The shared painter draws the same pixels into either format, and
        // a slot's size is its own.
        let [(rgba, ..), (wider, ..), (bgra, ..)] = slots;
        let context = executor.backend();
        let rgba_pixels = context.read_slot(rgba).expect("the RGBA slot reads back");
        let bgra_pixels = context.read_slot(bgra).expect("the BGRA slot reads back");
        let swizzled: Vec<u8> = bgra_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|[b, g, r, a]| [*r, *g, *b, *a])
            .collect();
        assert_eq!(swizzled, rgba_pixels, "both formats draw the same frame");
        assert_eq!(&rgba_pixels[..4], &[255, 0, 0, 255], "the square is red");
        assert!(
            rgba_pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|[r, g, b, _]| r == g && g == b && *r < 128),
            "the glyph inks"
        );
        assert_eq!(
            context
                .read_slot(wider)
                .expect("the wider slot reads back")
                .len(),
            48 * 16 * 4
        );
        assert!(matches!(
            context.read_slot(sender.mint_slot().expect("a slot id is left")),
            Err(EngineError::InvalidRenderTarget { .. })
        ));
    }
}
