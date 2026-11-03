//! Immutable per-target clip bindings; mask coordinates belong to the attachment.
//!
//! A mask is read only by fragments that pass their draw's hardware scissor
//! (`finalClipCoverage` reads zero outside the mask's extent). So a mask is
//! resolved over its clip's root bounds mapped into the attachment, cut to the
//! footprint of the draws that read it: the bounding box of their scissors,
//! clamped to the attachment. A partial frame's damage and a clip's enclosing
//! rectangle are such scissors, so neither pays for pixels no draw writes.
use super::GpuReplay;
use crate::{
    clip_chain::ClipChain,
    clip_mask::{ClipMaskPipeline, MaskEdge, MaskMapping, MaskNode},
    command_ir::{DrawRun, DrawSegment, ScissorRect},
    device_domain::PreparedCost,
    error::{EngineResult, GeometryError},
    pipeline_set::PipelineSet,
    resources::GpuResources,
};
use flui_foundation::geometry::Rect;
use wgpu::util::DeviceExt;

/// The attachment pixels, `left..right` by `top..bottom`, that the draws
/// reading one clip mask can write: the bounding box of their scissors,
/// clamped to the attachment. Never empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MaskFootprint {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl MaskFootprint {
    /// The pixels a draw under `scissor` can write in an `attachment` whose
    /// pixel (0, 0) is root pixel `origin`, clamped exactly as replay clamps
    /// it; `None` when the draw is skipped. A `None` scissor is the whole
    /// attachment.
    pub(crate) fn of_scissor(
        scissor: ScissorRect,
        attachment: (u32, u32),
        origin: (i64, i64),
    ) -> Option<Self> {
        let (x, y, w, h) = match scissor {
            Some((x, y, w, h)) => super::flush::clamp_scissor_to_attachment(
                x,
                y,
                w,
                h,
                attachment.0,
                attachment.1,
                origin,
            )?,
            None => (0, 0, attachment.0, attachment.1),
        };
        // The clamp keeps `x + w` within the attachment's `u32` width.
        (w > 0 && h > 0).then_some(Self {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        })
    }

    /// The pixels every piece of `run` that replay draws can write.
    pub(crate) fn of_run(
        segment: &DrawSegment,
        run: &DrawRun,
        attachment: (u32, u32),
        origin: (i64, i64),
    ) -> Option<Self> {
        let mut footprint = None;
        segment.visit_scissors(run, &mut |scissor| {
            footprint = Self::union(footprint, Self::of_scissor(scissor, attachment, origin));
        });
        footprint
    }

    /// The bounding box of both footprints.
    pub(crate) fn union(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (Some(a), Some(b)) => Some(Self {
                left: a.left.min(b.left),
                top: a.top.min(b.top),
                right: a.right.max(b.right),
                bottom: a.bottom.max(b.bottom),
            }),
            (a, None) => a,
            (None, b) => b,
        }
    }
}

/// The attachment pixels a clip mask is resolved over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaskCrop {
    /// No pixel a draw writes lies inside the clip's bounds: a 1×1 mask of
    /// zero coverage, so the draws write nothing (fail closed).
    Empty,
    /// `extent` pixels from attachment pixel `origin`.
    Region {
        origin: (u32, u32),
        extent: (u32, u32),
    },
}

impl MaskCrop {
    /// Whether a mask resolved over `self` answers a request for `other`.
    ///
    /// Outside its region a mask reads zero, which is the clip's coverage
    /// only outside the clip's bounds, so a mask serves exactly the requests
    /// whose region it contains.
    pub(crate) fn contains(self, other: Self) -> bool {
        match (self, other) {
            (_, Self::Empty) => true,
            (Self::Empty, Self::Region { .. }) => false,
            (
                Self::Region { origin, extent },
                Self::Region {
                    origin: inner,
                    extent: inner_extent,
                },
            ) => {
                let right = u64::from(origin.0) + u64::from(extent.0);
                let bottom = u64::from(origin.1) + u64::from(extent.1);
                origin.0 <= inner.0
                    && origin.1 <= inner.1
                    && u64::from(inner.0) + u64::from(inner_extent.0) <= right
                    && u64::from(inner.1) + u64::from(inner_extent.1) <= bottom
            }
        }
    }
}

/// The crop a mask is resolved over: `root_bounds` (the clip's root-space
/// bounds; `None` is unbounded) mapped into the attachment through the inverse
/// of `m` (attachment pixels to root pixels, with `determinant`), rounded out,
/// then cut to `footprint`, which lies inside the attachment. A `None`
/// footprint is a clip no draw reads through, which fails closed.
///
/// Cutting can only shrink the crop: wherever the mapped bounds round out past
/// the footprint (a f64 edge a hair past an integer), the footprint wins.
pub(crate) fn mask_crop(
    root_bounds: Option<Rect<f64>>,
    m: [f64; 6],
    determinant: f64,
    footprint: Option<MaskFootprint>,
) -> MaskCrop {
    let Some(footprint) = footprint else {
        return MaskCrop::Empty;
    };
    let mut crop = [
        f64::from(footprint.left),
        f64::from(footprint.top),
        f64::from(footprint.right),
        f64::from(footprint.bottom),
    ];
    if let Some(bounds) = root_bounds {
        if bounds.is_empty() {
            return MaskCrop::Empty;
        }
        let mut mapped = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for x in [bounds.left(), bounds.right()] {
            for y in [bounds.top(), bounds.bottom()] {
                let x = x - m[4];
                let y = y - m[5];
                let p = [
                    (m[3] * x - m[2] * y) / determinant,
                    (-m[1] * x + m[0] * y) / determinant,
                ];
                mapped[0] = mapped[0].min(p[0]);
                mapped[1] = mapped[1].min(p[1]);
                mapped[2] = mapped[2].max(p[0]);
                mapped[3] = mapped[3].max(p[1]);
            }
        }
        // `max`/`min` keep the footprint's edge where a mapped edge is NaN.
        crop = [
            mapped[0].floor().max(crop[0]),
            mapped[1].floor().max(crop[1]),
            mapped[2].ceil().min(crop[2]),
            mapped[3].ceil().min(crop[3]),
        ];
    }
    if crop[2] <= crop[0] || crop[3] <= crop[1] {
        return MaskCrop::Empty;
    }
    // Every edge lies within the footprint's `u32` pixels.
    MaskCrop::Region {
        origin: (crop[0] as u32, crop[1] as u32),
        extent: ((crop[2] - crop[0]) as u32, (crop[3] - crop[1]) as u32),
    }
}

impl GpuReplay {
    pub(super) fn prepare_unclipped_binding(
        &mut self,
        device: &wgpu::Device,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
    ) -> EngineResult<wgpu::BindGroup> {
        if let Some(cached) = &self.unclipped_binding
            && cached.size == self.uniform_size
            && cached.origin == self.attachment_origin
        {
            return Ok(cached.binding.clone());
        }
        let binding = self.create_target_binding(
            device,
            pipelines,
            resources,
            &self.dummy_mask_view,
            (0, 0),
            false,
        )?;
        self.unclipped_binding = Some(super::UnclippedBinding {
            size: self.uniform_size,
            origin: self.attachment_origin,
            binding: binding.clone(),
        });
        Ok(binding)
    }

    pub(super) fn create_target_binding(
        &self,
        device: &wgpu::Device,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        mask: &wgpu::TextureView,
        origin: (i32, i32),
        enabled: bool,
    ) -> EngineResult<wgpu::BindGroup> {
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: 32,
            cpu_bytes: 32,
            objects: 3,
        })?;
        let viewport = [
            self.uniform_size.0 as f32,
            self.uniform_size.1 as f32,
            self.attachment_origin.0 as f32,
            self.attachment_origin.1 as f32,
        ];
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Immutable clip viewport"),
            contents: bytemuck::cast_slice(&viewport),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let consumer = [origin.0, origin.1, i32::from(enabled), 0];
        let consumer_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Immutable clip consumer"),
            contents: bytemuck::cast_slice(&consumer),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Ok(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Immutable target clip binding"),
            layout: pipelines.viewport_bind_group_layout(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: consumer_buffer.as_entire_binding(),
                },
            ],
        }))
    }

    /// The binding through which the draws of `segment` under `clip` read
    /// their coverage, resolved over `footprint`, the attachment pixels those
    /// draws can write (see [`MaskFootprint`]).
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn prepare_clip_binding(
        &mut self,
        segment: &DrawSegment,
        clip: &ClipChain,
        scissor_covers_clip: bool,
        footprint: Option<MaskFootprint>,
        attachment_size: (u32, u32),
        device: &wgpu::Device,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<wgpu::BindGroup> {
        if clip.is_unclipped()
            || (scissor_covers_clip
                && segment.attachment_to_root == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
                && clip.integral_rect_scissor().is_some())
        {
            return self.prepare_unclipped_binding(device, pipelines, resources);
        }
        let mut m = segment.attachment_to_root;
        let (x, y) = self.attachment_origin;
        m[4] += m[0] * x as f64 + m[2] * y as f64;
        m[5] += m[1] * x as f64 + m[3] * y as f64;
        if m.iter().any(|v| !v.is_finite()) {
            return Err(GeometryError::NonFinite {
                context: "clip attachment mapping",
            }
            .into());
        }
        let determinant = m[0] * m[3] - m[1] * m[2];
        if !determinant.is_finite() || determinant == 0.0 {
            return Err(GeometryError::Unrepresentable {
                context: "clip attachment inverse",
            }
            .into());
        }
        // The crop is part of the request: it is known before the cache is
        // consulted, and a cached mask answers only a crop it contains.
        let crop = mask_crop(clip.root_bounds()?, m, determinant, footprint);
        let key = super::ClipBindingKey {
            clip: clip.clone(),
            mapping: m.map(f64::to_bits),
            attachment_size,
            uniform_size: self.uniform_size,
            origin: self.attachment_origin,
        };
        if let Some((_, binding)) = self
            .clip_bindings
            .get(&key)
            .and_then(|masks| masks.iter().find(|(held, _)| held.contains(crop)))
        {
            return Ok(binding.clone());
        }
        // Bound table growth, control bytes and bucket alignment before growth.
        // Eight buckets per admitted entry conservatively cover capacity slack,
        // and four slots per admitted mask cover a crop list's doubling.
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: 0,
            cpu_bytes: 8
                * (std::mem::size_of::<super::ClipBindingKey>()
                    + std::mem::size_of::<Vec<(MaskCrop, wgpu::BindGroup)>>()
                    + 32)
                + 4 * std::mem::size_of::<(MaskCrop, wgpu::BindGroup)>(),
            objects: 0,
        })?;
        let reserve_failed = |source| crate::error::EngineError::PreparedResourceAllocation {
            resource: "attachment clip bindings",
            source,
        };
        self.clip_bindings.try_reserve(1).map_err(reserve_failed)?;
        self.clip_bindings
            .entry(key.clone())
            .or_default()
            .try_reserve(1)
            .map_err(reserve_failed)?;
        let (empty, origin, extent) = match crop {
            MaskCrop::Empty => (true, (0, 0), (1, 1)),
            // Both lie within the attachment, a texture no wider than `i32`.
            MaskCrop::Region { origin, extent } => {
                (false, (origin.0 as i32, origin.1 as i32), extent)
            }
        };
        let scratch = 64 * std::mem::size_of::<MaskNode>()
            + 512 * std::mem::size_of::<MaskEdge>()
            + (2 * 512 + 1) * std::mem::size_of::<lyon::path::PathEvent>();
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: 0,
            cpu_bytes: scratch,
            objects: 3,
        })?;
        let mut tape = clip.lower()?;
        if empty {
            tape.nodes.clear();
            tape.edges.clear();
            tape.nodes.push(MaskNode {
                bounds: [0.0; 4],
                radii_x: [0.0; 4],
                radii_y: [0.0; 4],
                inverse: [1.0, 0.0, 0.0, 1.0],
                translation: [0.0; 4],
                meta: [0, 0, 1, 0],
                path_range: [0; 4],
            });
        }
        let edge_visits = tape
            .nodes
            .iter()
            .filter(|n| n.meta[0] == 3)
            .map(|n| n.path_range[1] as usize)
            .sum();
        let all_hard = tape.nodes.iter().all(|node| node.meta[2] == 1);
        let work = crate::clip_mask::work_units(extent, tape.nodes.len(), edge_visits, all_hard)?;
        segment.budget.admit_clip_work(work)?;
        let mapping = MaskMapping {
            attachment_to_root: [m[0] as f32, m[1] as f32, m[2] as f32, m[3] as f32],
            translation: [m[4] as f32, m[5] as f32, origin.0 as f32, origin.1 as f32],
            extent_counts: [extent.0, extent.1, 0, 0],
        };
        crate::clip_mask::validate_device_limits(device)?;
        if self.clip_mask_pipeline.is_none() {
            resources.reserve_prepared(PreparedCost {
                gpu_bytes: 0,
                cpu_bytes: 0,
                objects: 3,
            })?;
        }
        let pipeline = self
            .clip_mask_pipeline
            .get_or_insert_with(|| ClipMaskPipeline::new(device));
        let mask = pipeline.prepare(
            device,
            resources,
            encoder,
            &tape.nodes,
            &tape.edges,
            mapping,
            work,
        )?;
        let binding = self.create_target_binding(
            device,
            pipelines,
            resources,
            &mask.view,
            mask.origin,
            true,
        )?;
        self.clip_bindings
            .get_mut(&key)
            .expect("BUG: the crop list was reserved before the mask was prepared")
            .push((crop, binding.clone()));
        Ok(binding)
    }
}

#[cfg(test)]
mod tests {
    use super::{MaskCrop, MaskFootprint, mask_crop};
    use flui_foundation::geometry::Rect;

    const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    fn region(origin: (u32, u32), extent: (u32, u32)) -> MaskCrop {
        MaskCrop::Region { origin, extent }
    }

    /// The crop a mask is resolved over is the clip's mapped bounds cut to
    /// the footprint of the draws that read it: never larger than either, and
    /// empty (failing closed) when they miss each other or nothing reads it.
    #[test]
    fn mask_crop_is_cut_to_the_consumer_footprint() {
        let window = (6016, 3384);
        let full = |size| MaskFootprint::of_scissor(None, size, (0, 0));
        let damage =
            |x, y, w, h, size, origin| MaskFootprint::of_scissor(Some((x, y, w, h)), size, origin);
        /// The row, the clip's root bounds, the attachment-to-root mapping,
        /// the footprint and the crop expected.
        type Row = (
            &'static str,
            Option<Rect<f64>>,
            [f64; 6],
            Option<MaskFootprint>,
            MaskCrop,
        );
        let rows: [Row; 10] = [
            (
                "identity, the whole attachment reads it",
                Some(Rect::from_ltrb(10.5, 20.25, 100.5, 50.75)),
                IDENTITY,
                full((1024, 1024)),
                region((10, 20), (91, 31)),
            ),
            (
                "a partial frame's damage cuts a full-window clip",
                Some(Rect::from_ltrb(0.0, 0.0, 1024.0, 1024.0)),
                IDENTITY,
                damage(980, 980, 44, 44, (1024, 1024), (0, 0)),
                region((980, 980), (44, 44)),
            ),
            (
                "a footprint beside the clip",
                Some(Rect::from_ltrb(10.0, 10.0, 100.0, 100.0)),
                IDENTITY,
                damage(980, 980, 44, 44, (1024, 1024), (0, 0)),
                MaskCrop::Empty,
            ),
            (
                "no draw reads the mask",
                Some(Rect::from_ltrb(0.0, 0.0, 1024.0, 1024.0)),
                IDENTITY,
                None,
                MaskCrop::Empty,
            ),
            (
                "an empty clip",
                Some(Rect::ZERO),
                IDENTITY,
                full((1024, 1024)),
                MaskCrop::Empty,
            ),
            (
                "an unbounded clip takes the footprint",
                None,
                IDENTITY,
                damage(-8, 4, 20, 20, (16, 16), (0, 0)),
                region((0, 4), (12, 12)),
            ),
            (
                "a supersampled tile: half-pixel scale, translated",
                Some(Rect::from_ltrb(110.0, 210.0, 130.0, 230.0)),
                [0.5, 0.0, 0.0, 0.5, 100.0, 200.0],
                damage(0, 0, 40, 80, (80, 80), (0, 0)),
                region((20, 20), (20, 40)),
            ),
            (
                "an offscreen attachment whose pixel (0, 0) is root (300, 400)",
                Some(Rect::from_ltrb(310.0, 420.0, 350.0, 460.0)),
                [1.0, 0.0, 0.0, 1.0, 300.0, 400.0],
                damage(320, 400, 100, 100, (64, 64), (300, 400)),
                region((20, 20), (30, 40)),
            ),
            (
                "a f64 edge a hair past the footprint never grows the crop",
                Some(Rect::from_ltrb(0.0, 0.0, 1000.00001, 8.0)),
                IDENTITY,
                damage(0, 0, 1000, 8, (1024, 1024), (0, 0)),
                region((0, 0), (1000, 8)),
            ),
            (
                "a 6K window clip read by one pixel",
                Some(Rect::from_ltrb(0.0, 0.0, 6016.0, 3384.0)),
                IDENTITY,
                damage(3000, 1500, 1, 1, window, (0, 0)),
                region((3000, 1500), (1, 1)),
            ),
        ];
        let mut failures = Vec::new();
        for (row, bounds, m, footprint, expected) in rows {
            let determinant = m[0] * m[3] - m[1] * m[2];
            let crop = mask_crop(bounds, m, determinant, footprint);
            if crop != expected {
                failures.push(format!("{row}: {crop:?}, expected {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));

        // The 6K row is admitted at 64 membership tests where the whole
        // window, at one antialiased leaf, is over the frame's limit.
        let work = |footprint| match mask_crop(
            Some(Rect::from_ltrb(0.0, 0.0, 6016.0, 3384.0)),
            IDENTITY,
            1.0,
            footprint,
        ) {
            MaskCrop::Region { extent, .. } => {
                crate::clip_mask::work_units(extent, 1, 0, false).expect("in range")
            }
            MaskCrop::Empty => 0,
        };
        assert_eq!(work(damage(3000, 1500, 1, 1, window, (0, 0))), 64);
        assert_eq!(work(full(window)), 6016 * 3384 * 64);
    }

    /// A footprint is the attachment pixels replay's scissor clamp leaves,
    /// and a cached mask answers only the crops it contains.
    #[test]
    fn footprints_clamp_like_replay_and_crops_contain_their_requests() {
        assert_eq!(
            MaskFootprint::of_scissor(Some((2000, 0, 8, 8)), (1024, 1024), (0, 0)),
            None,
            "a scissor off the attachment is a skipped draw"
        );
        assert_eq!(
            MaskFootprint::of_scissor(Some((0, 0, 8, 8)), (0, 1024), (0, 0)),
            None
        );
        assert_eq!(MaskFootprint::of_scissor(None, (0, 16), (0, 0)), None);
        let a = MaskFootprint::of_scissor(Some((0, 0, 4, 4)), (64, 64), (0, 0));
        let b = MaskFootprint::of_scissor(Some((10, 20, 4, 4)), (64, 64), (0, 0));
        assert_eq!(
            MaskFootprint::union(a, b),
            MaskFootprint::of_scissor(Some((0, 0, 14, 24)), (64, 64), (0, 0))
        );
        assert_eq!(MaskFootprint::union(a, None), a);
        assert_eq!(MaskFootprint::union(None, b), b);

        let held = region((10, 10), (20, 20));
        let rows = [
            (held, true),
            (region((12, 12), (18, 18)), true),
            (region((12, 12), (19, 18)), false),
            (region((9, 10), (2, 2)), false),
            (MaskCrop::Empty, true),
        ];
        for (request, served) in rows {
            assert_eq!(held.contains(request), served, "{request:?}");
        }
        assert!(!MaskCrop::Empty.contains(held));
        assert!(MaskCrop::Empty.contains(MaskCrop::Empty));
    }
}
