//! Bounded rasterisation of an immutable root-to-leaf geometric clip tape.
//! Coverage is resolved once, after Boolean membership on a common sample grid.

use crate::{
    device_domain::PreparedCost,
    error::{EngineError, EngineResult, GeometryError},
    resources::GpuResources,
};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaskNode {
    pub(crate) bounds: [f32; 4],
    pub(crate) radii_x: [f32; 4],
    pub(crate) radii_y: [f32; 4],
    pub(crate) inverse: [f32; 4],
    pub(crate) translation: [f32; 4],
    /// Shape, operation, hard flag, fill rule. Corner order TL/TR/BR/BL.
    pub(crate) meta: [u32; 4],
    pub(crate) path_range: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaskEdge(pub(crate) [f32; 4]);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaskMapping {
    pub(crate) attachment_to_root: [f32; 4],
    /// Root translation XY, integer mask origin in attachment coordinates ZW.
    pub(crate) translation: [f32; 4],
    /// Mask width/height; prepare overwrites node count and sample count (1 hard, 64 AA).
    pub(crate) extent_counts: [u32; 4],
    /// Two boxes of mask pixels, each `[x0, x1) × [y0, y1)` as `[x0, y0, x1, y1]`,
    /// resolved to full coverage without
    /// sampling ([`MaskInterior`]); all zero when there are none. Prepare
    /// overwrites it from the tape.
    pub(crate) interior: [[u32; 4]; 2],
}

/// Mask pixels every sample of which every node of a tape decides as full
/// coverage does: inside each intersected node, outside each subtracted one.
///
/// The membership shader returns full coverage there without walking the
/// tape, so an antialiased clip pays its samples only along its curves. It is
/// derived from the tape and mapping the shader reads, in their `f32`
/// values, and is conservative: a pixel is in it only when its whole square
/// lies inside the node by more than the rounding of the shader's `f32`
/// arithmetic, so the early answer is the answer the samples would give.
///
/// It is the union of two boxes, so a rounded rectangle's interior is the
/// cross of its two arms and only its corners are sampled. The second box
/// is all zero when it adds no pixel to the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MaskInterior {
    boxes: [[u32; 4]; 2],
}

impl MaskInterior {
    /// Pixels in the interior: the union of its boxes.
    pub(crate) fn area(self) -> usize {
        let area = |[left, top, right, bottom]: [u32; 4]| {
            right.saturating_sub(left) as usize * bottom.saturating_sub(top) as usize
        };
        let [first, second] = self.boxes;
        let shared = [
            first[0].max(second[0]),
            first[1].max(second[1]),
            first[2].min(second[2]),
            first[3].min(second[3]),
        ];
        area(first) + area(second) - area(shared)
    }

    fn packed(self) -> [[u32; 4]; 2] {
        self.boxes
    }
}

/// The rounding slack of one membership test, relative to the magnitudes it
/// adds: a test rounds a handful of times in `f32` (`2^-24` each), so
/// sixteen units bound it several times over.
const MEMBERSHIP_SLACK: f64 = 1.0 / 1_048_576.0;

/// Mask pixels as a half-open box, signed so a node's interval can lie off
/// the mask before it is clamped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelBox {
    x: (i64, i64),
    y: (i64, i64),
}

impl PixelBox {
    fn area(self) -> i64 {
        (self.x.1 - self.x.0).max(0) * (self.y.1 - self.y.0).max(0)
    }

    fn intersect(self, other: Self) -> Self {
        Self {
            x: (self.x.0.max(other.x.0), self.x.1.min(other.x.1)),
            y: (self.y.0.max(other.y.0), self.y.1.min(other.y.1)),
        }
    }

    /// The largest of the four slabs of `self` beside `hole`, or `self` when
    /// they do not meet.
    fn without(self, hole: Self) -> Self {
        if self.intersect(hole).area() == 0 {
            return self;
        }
        [
            Self {
                x: (self.x.0, hole.x.0.min(self.x.1)),
                ..self
            },
            Self {
                x: (hole.x.1.max(self.x.0), self.x.1),
                ..self
            },
            Self {
                y: (self.y.0, hole.y.0.min(self.y.1)),
                ..self
            },
            Self {
                y: (hole.y.1.max(self.y.0), self.y.1),
                ..self
            },
        ]
        .into_iter()
        .max_by_key(|slab| slab.area())
        .expect("BUG: four slabs")
    }
}

/// One axis of the map from mask pixels to a node's local space as the
/// shader composes it, `local = scale · pixel + offset`, with the rounding
/// slack of its `f32` evaluation in local units.
#[derive(Clone, Copy)]
struct NodeAxis {
    scale: f64,
    offset: f64,
    slack: f64,
    extent: f64,
}

impl NodeAxis {
    /// `root_scale · (pixel + origin) + root_translation`, then
    /// `inverse_scale · root + node_translation`; `magnitude` bounds the
    /// node's own values on this axis (bounds, size, radii).
    fn try_new(
        root: (f64, f64),
        origin: f64,
        inverse: (f64, f64),
        extent: f64,
        magnitude: f64,
    ) -> Option<Self> {
        let ((root_scale, root_translation), (inverse_scale, node_translation)) = (root, inverse);
        let scale = inverse_scale * root_scale;
        let offset = inverse_scale * (root_scale * origin + root_translation) + node_translation;
        let slack = MEMBERSHIP_SLACK
            * (inverse_scale.abs()
                * (root_scale.abs() * (extent + origin.abs()) + root_translation.abs())
                + node_translation.abs()
                + magnitude);
        (scale != 0.0 && scale.is_finite() && offset.is_finite() && slack.is_finite()).then_some(
            Self {
                scale,
                offset,
                slack,
                extent,
            },
        )
    }

    /// The pixel coordinates `[s0, s1]` mapping onto local `[lo, hi]`,
    /// clamped a pixel past the mask so the casts below stay in range.
    fn pixels(self, lo: f64, hi: f64) -> (f64, f64) {
        let a = (lo - self.offset) / self.scale;
        let b = (hi - self.offset) / self.scale;
        let clamp = |v: f64| v.clamp(-1.0, self.extent + 1.0);
        (clamp(a.min(b)), clamp(a.max(b)))
    }

    /// Pixels whose whole square maps into `[lo, hi]` with the slack to
    /// spare on both sides.
    fn inside(self, lo: f64, hi: f64) -> (i64, i64) {
        let (lo, hi) = (lo + self.slack, hi - self.slack);
        if lo.is_nan() || hi.is_nan() || lo >= hi {
            return (0, 0);
        }
        let (s0, s1) = self.pixels(lo, hi);
        (s0.ceil() as i64, s1.floor() as i64)
    }

    /// Pixels some point of whose square maps within the slack of `[lo, hi]`.
    fn touching(self, lo: f64, hi: f64) -> (i64, i64) {
        let (s0, s1) = self.pixels(lo - self.slack, hi + self.slack);
        (s0.floor() as i64, s1.ceil() as i64)
    }
}

/// The [`MaskInterior`] of `nodes` resolved through `mapping`, or `None`
/// when it is empty or not a box this test can find: a mapping or a node
/// that rotates or skews, a path node, a non-finite value.
///
/// Two boxes are kept. An intersected rectangle keeps its bounds in both; a
/// rounded rectangle or a superellipse keeps the larger of its two arms clear
/// of every corner in the first and the other arm in the second (a corner
/// rejects a point only when it lies past the corner's centre on both axes),
/// so one window-sized rounded rectangle leaves the cross of its arms and
/// only the corners are sampled. A subtracted node removes its bounds from
/// each, keeping the largest slab beside them. Each box lies inside every
/// node on its own, so their union does; membership is a conjunction, so
/// the order the nodes are taken in changes which boxes are found, never
/// whether they are inside.
pub(crate) fn uniform_interior(nodes: &[MaskNode], mapping: &MaskMapping) -> Option<MaskInterior> {
    let [m0, m1, m2, m3] = mapping.attachment_to_root.map(f64::from);
    let [tx, ty, ox, oy] = mapping.translation.map(f64::from);
    let (width, height) = (mapping.extent_counts[0], mapping.extent_counts[1]);
    if nodes.is_empty() || m1 != 0.0 || m2 != 0.0 {
        return None;
    }
    let mask = PixelBox {
        x: (0, i64::from(width)),
        y: (0, i64::from(height)),
    };
    let (mut first, mut second) = (mask, mask);
    for node in nodes {
        let [i0, i1, i2, i3] = node.inverse.map(f64::from);
        if node.meta[0] > 2 || i1 != 0.0 || i2 != 0.0 {
            return None;
        }
        let [left, top, w, h] = node.bounds.map(f64::from);
        if !(left.is_finite() && top.is_finite() && w.is_finite() && h.is_finite()) {
            return None;
        }
        let (right, bottom) = (left + w, top + h);
        // Radii past `f32::MAX` cannot arrive as `f32`; a NaN radius is
        // refused before it can widen an arm.
        let rx = node.radii_x.map(f64::from);
        let ry = node.radii_y.map(f64::from);
        if rx.iter().chain(&ry).any(|r| !r.is_finite()) {
            return None;
        }
        let [rx, ry] = [rx, ry].map(|radii| radii.map(|r| r.max(0.0)));
        let largest = |radii: [f64; 4]| radii.into_iter().fold(0.0, f64::max);
        let [nx, ny, ..] = node.translation.map(f64::from);
        let x = NodeAxis::try_new(
            (m0, tx),
            ox,
            (i0, nx),
            f64::from(width),
            left.abs() + w.abs() + largest(rx),
        )?;
        let y = NodeAxis::try_new(
            (m3, ty),
            oy,
            (i3, ny),
            f64::from(height),
            top.abs() + h.abs() + largest(ry),
        )?;
        if node.meta[1] == 1 {
            let hole = PixelBox {
                x: x.touching(left, right),
                y: y.touching(top, bottom),
            };
            first = first.without(hole);
            second = second.without(hole);
        } else {
            let arm = |l: f64, t: f64, r: f64, b: f64| PixelBox {
                x: x.inside(l, r),
                y: y.inside(t, b),
            };
            let (larger, other) = if node.meta[0] == 0 {
                let bounds = arm(left, top, right, bottom);
                (bounds, bounds)
            } else {
                // Corners in order top-left, top-right, bottom-right, bottom-left.
                let horizontal = arm(
                    left,
                    top + ry[0].max(ry[1]),
                    right,
                    bottom - ry[2].max(ry[3]),
                );
                let vertical = arm(
                    left + rx[0].max(rx[3]),
                    top,
                    right - rx[1].max(rx[2]),
                    bottom,
                );
                if vertical.area() > horizontal.area() {
                    (vertical, horizontal)
                } else {
                    (horizontal, vertical)
                }
            };
            first = first.intersect(larger);
            second = second.intersect(other);
        }
        if first.area() == 0 && second.area() == 0 {
            return None;
        }
    }
    if first.area() == 0 {
        (first, second) = (second, first);
    }
    // A second box inside the first adds nothing to sample less.
    if second.area() == 0 || first.intersect(second) == second {
        second = PixelBox {
            x: (0, 0),
            y: (0, 0),
        };
    }
    // Every bound lies in `[0, extent]`: each box started as the mask and
    // only shrank, and an empty second box is all zero.
    let bound = |v: i64| u32::try_from(v).expect("BUG: the interior lies within the mask");
    let packed = |pixels: PixelBox| {
        [
            bound(pixels.x.0),
            bound(pixels.y.0),
            bound(pixels.x.1),
            bound(pixels.y.1),
        ]
    };
    Some(MaskInterior {
        boxes: [packed(first), packed(second)],
    })
}

pub(crate) struct PreparedClipMask {
    // Not pooled: encoded wgpu commands retain the immutable allocation.
    _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) origin: (i32, i32),
}

#[derive(Clone, Copy)]
#[repr(usize)]
enum MembershipKind {
    Rect,
    Curved,
    General,
}

pub(crate) struct ClipMaskPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    // Rect-only, curves (with rectangles), general (including paths).
    // Bounded by geometry kind, never by tape contents or sample count.
    pipelines: [Option<wgpu::RenderPipeline>; 3],
}

fn limit(resource: &'static str, requested: usize, cap: usize) -> EngineError {
    EngineError::PreparedResourceLimit {
        resource,
        requested,
        limit: cap,
    }
}

/// Reject insufficient capabilities before creating the clip pipeline or buffers.
pub(crate) fn validate_device_limits(device: &wgpu::Device) -> EngineResult<()> {
    let limits = device.limits();
    if limits.max_uniform_buffers_per_shader_stage < 3 {
        return Err(limit(
            "clip fragment uniform buffers",
            3,
            limits.max_uniform_buffers_per_shader_stage as usize,
        ));
    }
    if limits.max_uniform_buffer_binding_size < 8192 {
        return Err(limit(
            "clip uniform binding bytes",
            8192,
            usize::try_from(limits.max_uniform_buffer_binding_size).unwrap_or(usize::MAX),
        ));
    }
    if limits.max_buffer_size < 8192 {
        return Err(limit(
            "clip uniform buffer bytes",
            8192,
            usize::try_from(limits.max_buffer_size).unwrap_or(usize::MAX),
        ));
    }
    Ok(())
}

/// Conservative membership count: one sample for all-hard chains, otherwise
/// 64 correlated samples for mixed or antialiased chains, each testing every
/// node and path edge, for every pixel outside `interior`; one unit for each
/// of the `interior` pixels the shader answers without sampling.
/// The caller must admit this cumulatively, not independently per mask.
pub(crate) fn work_units(
    extent: (u32, u32),
    interior: usize,
    nodes: usize,
    edges: usize,
    all_hard: bool,
) -> EngineResult<usize> {
    (extent.0 as usize)
        .checked_mul(extent.1 as usize)
        .and_then(|area| area.checked_sub(interior))
        .and_then(|v| v.checked_mul(if all_hard { 1 } else { 64 }))
        .and_then(|v| {
            nodes
                .checked_add(edges)
                .and_then(|n| v.checked_mul(n.max(1)))
        })
        .and_then(|v| v.checked_add(interior))
        .ok_or_else(|| limit("clip membership work", usize::MAX, usize::MAX - 1))
}

/// The membership work of resolving `mapping`'s extent through `nodes`, and
/// the interior the shader answers without sampling. The caller admits
/// exactly this; [`ClipMaskPipeline::prepare`] derives it again from the same
/// tape and mapping and refuses more than was admitted.
pub(crate) fn membership_work(
    nodes: &[MaskNode],
    mapping: &MaskMapping,
) -> EngineResult<(usize, Option<MaskInterior>)> {
    // Ranges may share edges. Charge traversal, not just distinct storage.
    let edge_visits = nodes
        .iter()
        .filter(|node| node.meta[0] == 3)
        .try_fold(0_usize, |sum, node| {
            sum.checked_add(node.path_range[1] as usize)
        })
        .ok_or_else(|| limit("clip edge traversal", usize::MAX, usize::MAX - 1))?;
    let all_hard = nodes.iter().all(|node| node.meta[2] == 1);
    let interior = uniform_interior(nodes, mapping);
    let work = work_units(
        (mapping.extent_counts[0], mapping.extent_counts[1]),
        interior.map_or(0, MaskInterior::area),
        nodes.len(),
        edge_visits,
        all_hard,
    )?;
    Ok((work, interior))
}

impl ClipMaskPipeline {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let uniform_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Clip Mask Tape Layout"),
            entries: &[
                uniform_entry(0),
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Clip Mask Pipeline Layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Clip Mask Membership"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/clip_mask.wgsl").into()),
        });
        Self {
            layout,
            pipeline_layout,
            shader,
            pipelines: [None, None, None],
        }
    }

    fn create_pipeline(&self, device: &wgpu::Device, kind: MembershipKind) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Clip Mask Resolve"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[
                        (
                            "HAS_PATHS",
                            if matches!(kind, MembershipKind::General) {
                                1.0
                            } else {
                                0.0
                            },
                        ),
                        (
                            "HAS_CURVES",
                            if matches!(kind, MembershipKind::Rect) {
                                0.0
                            } else {
                                1.0
                            },
                        ),
                    ],
                    ..Default::default()
                },
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    #[expect(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        nodes: &[MaskNode],
        edges: &[MaskEdge],
        mut mapping: MaskMapping,
        admitted_work: usize,
    ) -> EngineResult<PreparedClipMask> {
        validate_device_limits(device)?;
        if nodes.len() > 64 {
            return Err(limit("clip mask nodes", nodes.len(), 64));
        }
        if edges.len() > 512 {
            return Err(limit("clip mask edges", edges.len(), 512));
        }
        let extent = (mapping.extent_counts[0], mapping.extent_counts[1]);
        if extent.0 == 0 || extent.1 == 0 {
            return Err(EngineError::InvalidTargetSize {
                width: extent.0,
                height: extent.1,
            });
        }
        let max_extent = device.limits().max_texture_dimension_2d;
        if extent.0 > max_extent || extent.1 > max_extent {
            return Err(limit(
                "clip mask extent",
                extent.0.max(extent.1) as usize,
                max_extent as usize,
            ));
        }
        if extent.0 > 16_384 || extent.1 > 16_384 {
            return Err(limit(
                "clip numeric attachment extent",
                extent.0.max(extent.1) as usize,
                16_384,
            ));
        }
        let all_hard = nodes.iter().all(|node| node.meta[2] == 1);
        let (work, interior) = membership_work(nodes, &mapping)?;
        if work > admitted_work {
            return Err(limit("clip membership work", work, admitted_work));
        }
        if !mapping
            .attachment_to_root
            .iter()
            .chain(&mapping.translation)
            .all(|v| v.is_finite())
        {
            return Err(GeometryError::NonFinite {
                context: "clip mask mapping",
            }
            .into());
        }
        for &value in mapping
            .attachment_to_root
            .iter()
            .chain(&mapping.translation)
        {
            crate::clip_geometry::pack_clip_value(f64::from(value), "clip mask mapping range")?;
        }
        let mut origin = [0_i32; 2];
        for (i, v) in mapping.translation[2..].iter().copied().enumerate() {
            if v.fract() != 0.0
                || f64::from(v) < f64::from(i32::MIN)
                || f64::from(v) > f64::from(i32::MAX)
            {
                return Err(GeometryError::Unrepresentable {
                    context: "clip mask integer origin",
                }
                .into());
            }
            origin[i] = v as i32;
            if i64::from(origin[i]) + i64::from(mapping.extent_counts[i]) > i64::from(i32::MAX) {
                return Err(GeometryError::InvalidExtent.into());
            }
        }
        for node in nodes {
            for &value in node
                .bounds
                .iter()
                .chain(&node.radii_x)
                .chain(&node.radii_y)
                .chain(&node.inverse)
                .chain(&node.translation)
            {
                crate::clip_geometry::pack_clip_value(f64::from(value), "clip mask node range")?;
            }
            if !node
                .bounds
                .iter()
                .chain(&node.radii_x)
                .chain(&node.radii_y)
                .chain(&node.inverse)
                .chain(&node.translation)
                .all(|v| v.is_finite())
            {
                return Err(GeometryError::NonFinite {
                    context: "clip mask node",
                }
                .into());
            }
            if node.meta[0] > 3 || node.meta[1] > 1 || node.meta[2] > 1 || node.meta[3] > 1 {
                return Err(GeometryError::Unrepresentable {
                    context: "clip mask node tag",
                }
                .into());
            }
            if node.bounds[2] < 0.0
                || node.bounds[3] < 0.0
                || !(node.bounds[0] + node.bounds[2]).is_finite()
                || !(node.bounds[1] + node.bounds[3]).is_finite()
            {
                return Err(GeometryError::InvalidExtent.into());
            }
            if node.radii_x.iter().chain(&node.radii_y).any(|v| *v < 0.0) {
                return Err(GeometryError::InvalidRadius.into());
            }
            let end = node.path_range[0].checked_add(node.path_range[1]);
            if node.meta[0] == 3 && end.is_none_or(|end| end as usize > edges.len()) {
                return Err(GeometryError::Unrepresentable {
                    context: "clip mask path range",
                }
                .into());
            }
        }
        if edges
            .iter()
            .any(|edge| edge.0.iter().any(|v| !v.is_finite()))
        {
            return Err(GeometryError::NonFinite {
                context: "clip mask path edge",
            }
            .into());
        }
        for edge in edges {
            for &value in &edge.0 {
                crate::clip_geometry::pack_clip_value(f64::from(value), "clip mask edge range")?;
            }
        }
        mapping.extent_counts[2] = u32::try_from(nodes.len())
            .map_err(|_| limit("clip node count", nodes.len(), u32::MAX as usize))?;
        mapping.extent_counts[3] = if all_hard { 1 } else { 64 };
        mapping.interior = interior.map_or([[0; 4]; 2], MaskInterior::packed);
        // Fixed uniform arrays work on baseline WebGL2 without storage buffers.
        let payload = 7168 + 8192 + std::mem::size_of::<MaskMapping>();
        let texture_bytes = (extent.0 as usize)
            .checked_mul(extent.1 as usize)
            .ok_or_else(|| limit("clip mask bytes", usize::MAX, usize::MAX - 1))?;
        let gpu_bytes = payload
            .checked_add(texture_bytes)
            .ok_or_else(|| limit("clip prepared bytes", usize::MAX, usize::MAX - 1))?;
        resources.reserve_prepared(PreparedCost {
            gpu_bytes,
            cpu_bytes: payload,
            objects: 6,
        })?;
        let kind = if nodes.iter().any(|node| node.meta[0] == 3) {
            MembershipKind::General
        } else if nodes.iter().any(|node| node.meta[0] != 0) {
            MembershipKind::Curved
        } else {
            MembershipKind::Rect
        };
        if self.pipelines[kind as usize].is_none() {
            resources.reserve_prepared(PreparedCost {
                gpu_bytes: 0,
                cpu_bytes: 0,
                objects: 1,
            })?;
            self.pipelines[kind as usize] = Some(self.create_pipeline(device, kind));
        }
        let mut padded_nodes = [0_u8; 7168];
        let mut padded_edges = [0_u8; 8192];
        let node_bytes = bytemuck::cast_slice(nodes);
        let edge_bytes = bytemuck::cast_slice(edges);
        padded_nodes[..node_bytes.len()].copy_from_slice(node_bytes);
        padded_edges[..edge_bytes.len()].copy_from_slice(edge_bytes);
        // Every allocation below follows admission; no pooled mask can be overwritten.
        let buffer = |label, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        let nodes_buffer = buffer(
            "Immutable Clip Nodes",
            &padded_nodes,
            wgpu::BufferUsages::UNIFORM,
        );
        let edges_buffer = buffer(
            "Immutable Clip Edges",
            &padded_edges,
            wgpu::BufferUsages::UNIFORM,
        );
        let uniform = buffer(
            "Clip Mask Mapping",
            bytemuck::bytes_of(&mapping),
            wgpu::BufferUsages::UNIFORM,
        );
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Clip Mask Tape"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: nodes_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: edges_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Immutable Clip Coverage"),
            size: wgpu::Extent3d {
                width: extent.0,
                height: extent.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clip Boolean Membership Resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(
                self.pipelines[kind as usize]
                    .as_ref()
                    .expect("BUG: clip pipeline admitted"),
            );
            pass.set_bind_group(0, &binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(PreparedClipMask {
            _texture: texture,
            view,
            origin: (origin[0], origin[1]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MaskInterior, MaskMapping, MaskNode, membership_work, uniform_interior, work_units,
    };

    const RECT: u32 = 0;
    const RRECT: u32 = 1;
    const SUPERELLIPSE: u32 = 2;
    const PATH: u32 = 3;

    fn node(shape: u32, bounds: [f32; 4], radii_x: [f32; 4], radii_y: [f32; 4]) -> MaskNode {
        MaskNode {
            bounds,
            radii_x,
            radii_y,
            inverse: [1.0, 0.0, 0.0, 1.0],
            translation: [0.0; 4],
            meta: [shape, 0, 0, 0],
            path_range: [0; 4],
        }
    }

    fn hard(mut node: MaskNode) -> MaskNode {
        node.meta[2] = 1;
        node
    }

    fn subtracted(mut node: MaskNode) -> MaskNode {
        node.meta[1] = 1;
        node
    }

    fn mapped(
        attachment_to_root: [f32; 4],
        translation: [f32; 4],
        extent: (u32, u32),
    ) -> MaskMapping {
        MaskMapping {
            attachment_to_root,
            translation,
            extent_counts: [extent.0, extent.1, 0, 0],
            interior: [[0; 4]; 2],
        }
    }

    fn identity(extent: (u32, u32)) -> MaskMapping {
        mapped([1.0, 0.0, 0.0, 1.0], [0.0; 4], extent)
    }

    fn interior(left: u32, top: u32, right: u32, bottom: u32) -> MaskInterior {
        MaskInterior {
            boxes: [[left, top, right, bottom], [0; 4]],
        }
    }

    impl MaskInterior {
        /// This interior with `[left, right) × [top, bottom)` as its second box.
        fn and(mut self, left: u32, top: u32, right: u32, bottom: u32) -> Self {
            self.boxes[1] = [left, top, right, bottom];
            self
        }
    }

    /// The samples of one mask pixel that pass every node and how many were
    /// taken, evaluated as `shaders/clip_mask.wgsl` evaluates them, in `f32`.
    fn shader_coverage(nodes: &[MaskNode], mapping: &MaskMapping, px: u32, py: u32) -> (u32, u32) {
        let samples = if nodes.iter().all(|n| n.meta[2] == 1) {
            1
        } else {
            64
        };
        let [m0, m1, m2, m3] = mapping.attachment_to_root;
        let [tx, ty, ox, oy] = mapping.translation;
        let root = |x: f32, y: f32| {
            let shifted = (x + ox, y + oy);
            (
                m0 * shifted.0 + m2 * shifted.1 + tx,
                m1 * shifted.0 + m3 * shifted.1 + ty,
            )
        };
        let contains = |node: &MaskNode, point: (f32, f32)| {
            let [i0, i1, i2, i3] = node.inverse;
            let local = (
                i0 * point.0 + i2 * point.1 + node.translation[0],
                i1 * point.0 + i3 * point.1 + node.translation[1],
            );
            assert_ne!(node.meta[0], PATH, "the mirror evaluates analytic nodes");
            let lo = (node.bounds[0], node.bounds[1]);
            let hi = (lo.0 + node.bounds[2], lo.1 + node.bounds[3]);
            if local.0 < lo.0 || local.1 < lo.1 || local.0 >= hi.0 || local.1 >= hi.1 {
                return false;
            }
            if node.meta[0] == RECT {
                return true;
            }
            (0..4).all(|corner| {
                let (right, bottom) = (corner == 1 || corner == 2, corner >= 2);
                let radius = (node.radii_x[corner], node.radii_y[corner]);
                if radius.0 <= 0.0 || radius.1 <= 0.0 {
                    return true;
                }
                let centre = (
                    if right {
                        hi.0 - radius.0
                    } else {
                        lo.0 + radius.0
                    },
                    if bottom {
                        hi.1 - radius.1
                    } else {
                        lo.1 + radius.1
                    },
                );
                let delta = (
                    if right {
                        local.0 - centre.0
                    } else {
                        centre.0 - local.0
                    },
                    if bottom {
                        local.1 - centre.1
                    } else {
                        centre.1 - local.1
                    },
                );
                if delta.0 <= 0.0 || delta.1 <= 0.0 {
                    return true;
                }
                let ratio = (delta.0 / radius.0, delta.1 / radius.1);
                let squared = (ratio.0 * ratio.0, ratio.1 * ratio.1);
                let norm = if node.meta[0] == SUPERELLIPSE {
                    squared.0 * squared.0 + squared.1 * squared.1
                } else {
                    squared.0 + squared.1
                };
                norm <= 1.0
            })
        };
        let (x, y) = (px as f32, py as f32);
        let center = root(x + 0.5, y + 0.5);
        let covered = (0..samples)
            .filter(|s| {
                let (sx, sy) = ((s % 8) as f32, (s / 8) as f32);
                let sample = root(x + (sx + 0.5) / 8.0, y + (sy + 0.5) / 8.0);
                nodes.iter().all(|n| {
                    let hit = contains(n, if n.meta[2] != 0 { center } else { sample });
                    if n.meta[1] == 1 { !hit } else { hit }
                })
            })
            .count() as u32;
        (covered, samples)
    }

    /// The interior is the box the shader may answer without sampling: every
    /// pixel in it resolves to full coverage when sampled, edges at fractional
    /// positions, under a scaled, offset or flipped mapping, through corners,
    /// subtracted holes and nested leaves. Rotation, skew, paths and empty or
    /// non-finite nodes have none, and fall back to sampling every pixel.
    #[test]
    fn the_uniform_interior_is_where_every_sample_is_inside() {
        let r = |v: f32| [v; 4];
        let window = node(RRECT, [0.0, 0.0, 1024.0, 1024.0], r(32.0), r(32.0));
        let mut flipped = hard(node(RECT, [0.0, 0.0, 32.0, 64.0], r(0.0), r(0.0)));
        flipped.inverse = [-1.0, 0.0, 0.0, 1.0];
        flipped.translation = [64.0, 0.0, 0.0, 0.0];
        let mut skewed = node(RECT, [0.0, 0.0, 64.0, 64.0], r(0.0), r(0.0));
        skewed.inverse = [1.0, 0.0, 0.5, 1.0];
        let mut path = node(PATH, [0.0, 0.0, 64.0, 64.0], r(0.0), r(0.0));
        path.path_range = [0, 0, 0, 0];
        let small = (64, 64);
        /// The row, the tape, its mapping, the interior expected.
        type Row = (
            &'static str,
            Vec<MaskNode>,
            MaskMapping,
            Option<MaskInterior>,
        );
        let rows: Vec<Row> = vec![
            (
                "a window-sized rounded rectangle keeps the cross of its arms",
                vec![window; 16],
                identity((1024, 1024)),
                Some(interior(1, 33, 1023, 991).and(33, 1, 991, 1023)),
            ),
            (
                "a hard rectangle at fractional edges",
                vec![hard(node(RECT, [8.5, 8.25, 47.0, 40.5], r(0.0), r(0.0)))],
                identity(small),
                Some(interior(9, 9, 55, 48)),
            ),
            (
                "a fractional root translation",
                vec![node(RECT, [8.0, 8.0, 48.0, 48.0], r(0.0), r(0.0))],
                mapped([1.0, 0.0, 0.0, 1.0], [0.25, 0.75, 0.0, 0.0], small),
                Some(interior(8, 8, 55, 55)),
            ),
            (
                "a scaled root and a mask origin off the attachment's corner",
                vec![node(RECT, [60.0, 30.0, 40.0, 20.0], r(0.0), r(0.0))],
                mapped([0.5, 0.0, 0.0, 0.5], [0.0, 0.0, 100.0, 40.0], (128, 64)),
                Some(interior(21, 21, 99, 59)),
            ),
            (
                "a flipped node",
                vec![flipped],
                identity(small),
                Some(interior(33, 1, 63, 63)),
            ),
            (
                "a tall corner keeps the vertical arm",
                vec![node(
                    RRECT,
                    [0.0, 0.0, 64.0, 64.0],
                    r(4.0),
                    [24.0, 2.0, 2.0, 2.0],
                )],
                identity(small),
                Some(interior(5, 1, 59, 63).and(1, 25, 63, 61)),
            ),
            (
                "a superellipse",
                vec![node(SUPERELLIPSE, [0.0, 0.0, 64.0, 64.0], r(16.0), r(16.0))],
                identity(small),
                Some(interior(1, 17, 63, 47).and(17, 1, 47, 63)),
            ),
            (
                "nested leaves intersect",
                vec![
                    hard(node(RECT, [0.0, 0.0, 64.0, 64.0], r(0.0), r(0.0))),
                    node(RRECT, [8.0, 8.0, 48.0, 48.0], r(8.0), r(8.0)),
                ],
                identity(small),
                Some(interior(9, 17, 55, 47).and(17, 9, 47, 55)),
            ),
            (
                "a subtracted rectangle keeps the largest slab beside it",
                vec![
                    hard(node(RECT, [0.0, 0.0, 64.0, 64.0], r(0.0), r(0.0))),
                    hard(subtracted(node(
                        RECT,
                        [8.0, 40.0, 48.0, 16.0],
                        r(0.0),
                        r(0.0),
                    ))),
                ],
                identity(small),
                Some(interior(1, 1, 63, 39)),
            ),
            (
                "a subtracted node over the whole interior leaves none",
                vec![
                    node(RECT, [0.0, 0.0, 64.0, 64.0], r(0.0), r(0.0)),
                    subtracted(node(RECT, [-8.0, -8.0, 80.0, 80.0], r(0.0), r(0.0))),
                ],
                identity(small),
                None,
            ),
            (
                "a rotated mapping",
                vec![node(RECT, [0.0, 0.0, 64.0, 64.0], r(0.0), r(0.0))],
                mapped([0.6, 0.8, -0.8, 0.6], [0.0; 4], small),
                None,
            ),
            ("a skewed node", vec![skewed], identity(small), None),
            ("a path node", vec![path], identity(small), None),
            (
                "the empty clip",
                vec![hard(node(RECT, [0.0; 4], r(0.0), r(0.0)))],
                identity(small),
                None,
            ),
            (
                "a non-finite bound",
                vec![node(RECT, [f32::NAN, 0.0, 64.0, 64.0], r(0.0), r(0.0))],
                identity(small),
                None,
            ),
            (
                "a non-finite mapping",
                vec![node(RECT, [0.0, 0.0, 64.0, 64.0], r(0.0), r(0.0))],
                mapped([f32::INFINITY, 0.0, 0.0, 1.0], [0.0; 4], small),
                None,
            ),
        ];
        let mut failures = Vec::new();
        for (row, nodes, mapping, expected) in &rows {
            let found = uniform_interior(nodes, mapping);
            if found != *expected {
                failures.push(format!("{row}: {found:?}, expected {expected:?}"));
                continue;
            }
            let Some(found) = found else { continue };
            // Every pixel of a small box, the border of a large one, for
            // each box that holds any.
            let mut pixels = found
                .packed()
                .into_iter()
                .filter(|[left, top, right, bottom]| left < right && top < bottom)
                .flat_map(|[left, top, right, bottom]| -> Vec<(u32, u32)> {
                    if (right - left) * (bottom - top) <= 4096 {
                        (top..bottom)
                            .flat_map(|y| (left..right).map(move |x| (x, y)))
                            .collect()
                    } else {
                        (left..right)
                            .flat_map(|x| [(x, top), (x, bottom - 1)])
                            .chain((top..bottom).flat_map(|y| [(left, y), (right - 1, y)]))
                            .collect()
                    }
                });

            if let Some((x, y, covered, samples)) = pixels.find_map(|(x, y)| {
                let (covered, samples) = shader_coverage(nodes, mapping, x, y);
                (covered != samples).then_some((x, y, covered, samples))
            }) {
                failures.push(format!(
                    "{row}: pixel ({x}, {y}) of the interior resolves {covered}/{samples}"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The interior is charged one unit a pixel; only the pixels around it pay
    /// their samples. A window-sized antialiased clip at sixteen leaves fits
    /// the frame's 1e9 work limit at its edge cost.
    #[test]
    fn the_interior_is_charged_once_a_pixel() {
        let window = node(RRECT, [0.0, 0.0, 1024.0, 1024.0], [32.0; 4], [32.0; 4]);
        let nodes = vec![window; 16];
        let mapping = identity((1024, 1024));
        // The cross of the two arms: only the four 32 × 32 corners sample.
        let interior_area = 2 * 1022 * 958 - 958 * 958;
        let sampled = 1024 * 1024 - interior_area;
        assert_eq!(
            membership_work(&nodes, &mapping).expect("in range"),
            (
                sampled * 64 * 16 + interior_area,
                Some(interior(1, 33, 1023, 991).and(33, 1, 991, 1023))
            ),
        );
        assert_eq!(
            work_units((1024, 1024), 0, 16, 0, false).expect("in range"),
            1_073_741_824,
            "the whole mask sampled is over the limit"
        );
        assert_eq!(
            work_units((1024, 1024), 1024 * 1024, 16, 0, false).expect("in range"),
            1024 * 1024,
        );

        // A 6K window under one antialiased rounded rectangle of radius 12:
        // 1,302,921,216 units sampled whole, refused on every full frame;
        // the vertical arm is the wider one, and the horizontal one adds its
        // strips along the short sides.
        let window = node(RRECT, [0.0, 0.0, 6016.0, 3384.0], [12.0; 4], [12.0; 4]);
        let mapping = identity((6016, 3384));
        assert_eq!(
            work_units((6016, 3384), 0, 1, 0, false).expect("in range"),
            1_302_921_216,
        );
        assert_eq!(
            membership_work(&[window], &mapping).expect("in range"),
            (
                21_578_580,
                Some(interior(13, 1, 6003, 3383).and(1, 13, 6015, 3371))
            ),
        );
    }
}
