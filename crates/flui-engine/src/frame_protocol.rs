//! The frame protocol the windowed renderer and the headless retained
//! capture share (ADR-0087 §4): the damage owed, the retained target that
//! holds the last frame, and the sequence that turns a [`FramePlan`](crate::damage::FramePlan) into
//! GPU work — pick the target, clear it unless the frame is partial, record
//! the content, blit a retained frame to the surface, commit.
//!
//! Only the steps that genuinely differ between a swapchain and a readback
//! texture ([`FrameSteps`](crate::frame_protocol::FrameSteps)) live with each caller; everything a readback test
//! pins about partial frames runs through this one implementation.

use crate::device_domain::DeviceDomain;
use flui_foundation::geometry::Rect;
use std::sync::Arc;

use crate::damage::{DamageTracker, FramePlan, plan_frame};
use crate::error::EngineResult;
use crate::retained_target::RetainedTarget;

/// The colour a frame starts from: the full clear pass and the clear a
/// partial frame draws inside its damage both paint it, so the background a
/// partial frame repaints is the one a full frame shows.
pub(crate) const BACKGROUND: flui_painting::styling::Color = flui_painting::styling::Color::WHITE;

/// [`BACKGROUND`] as a render pass's clear value. The channels map straight
/// through, as the painter maps a fill's colour; `partial_equals_full_inside_damage`
/// compares the two backgrounds pixel for pixel.
pub(crate) fn background_clear_value() -> wgpu::Color {
    let channel = |value: u8| f64::from(value) / 255.0;
    wgpu::Color {
        r: channel(BACKGROUND.r),
        g: channel(BACKGROUND.g),
        b: channel(BACKGROUND.b),
        a: channel(BACKGROUND.a),
    }
}

/// The parts of a frame that depend on where it is presented.
pub(crate) trait FrameSteps {
    /// Clears `view` to [`BACKGROUND`] and submits the pass.
    fn clear(&mut self, view: &wgpu::TextureView) -> EngineResult<()>;

    /// Records and submits the frame's content into `(view, texture)`;
    /// `retained` says the target is the retained texture rather than the
    /// surface, `partial` the damage a partial frame scissors to. Returns
    /// whether an advanced shape straddled that damage, in which case the
    /// next frame renders in full.
    ///
    /// # Errors
    /// The content pass's failure, which is the frame's.
    fn content(
        &mut self,
        view: &wgpu::TextureView,
        texture: &wgpu::Texture,
        retained: bool,
        partial: Option<Rect<f64>>,
    ) -> EngineResult<bool>;

    /// Copies the whole retained texture onto the surface's view.
    fn blit(&mut self, retained: &wgpu::Texture, surface: &wgpu::TextureView) -> EngineResult<()>;
}

/// A renderer's damage, its retained target and the one-frame promotion to a
/// full repaint.
#[derive(Debug)]
pub(crate) struct FrameProtocol {
    damage: DamageTracker,
    retained: RetainedTarget,
    /// When set, the next [`Self::plan`] promotes its damage to a full
    /// repaint, and clears the flag.
    ///
    /// Set when a partial frame found an advanced shape straddling its
    /// damage edge (such a shape composites over its whole device bounds
    /// with no scissor, so its out-of-damage slice blends over the previous
    /// frame for one frame), and after a frame rendered outside the damage
    /// protocol ([`Self::end_unmanaged`]).
    force_full_next_frame: bool,
}

impl Default for FrameProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameProtocol {
    /// A protocol whose first frame renders in full.
    pub(crate) fn new() -> Self {
        Self {
            damage: DamageTracker::new(),
            retained: RetainedTarget::default(),
            force_full_next_frame: false,
        }
    }

    /// Adds `rect` to the damage owed.
    pub(crate) fn mark_dirty(&mut self, rect: Rect<f64>) {
        self.damage.mark_dirty(rect);
    }

    /// Owes a full repaint.
    pub(crate) fn mark_full_repaint(&mut self) {
        self.damage.mark_full_repaint();
    }

    /// Whether any damage is owed.
    #[must_use]
    pub(crate) fn has_damage(&self) -> bool {
        self.damage.has_damage()
    }

    /// The surface was resized, reconfigured or rebuilt: every pixel is
    /// owed, and the retained target no longer matches what it shows.
    pub(crate) fn surface_changed(&mut self) {
        self.damage.mark_full_repaint();
        self.retained.invalidate();
    }

    /// Drops the retained target's texture (its device is gone, or the
    /// surface was released and its memory goes with it).
    pub(crate) fn release_target(&mut self) {
        self.retained.release();
    }

    /// The plan for the next frame, applying a pending promotion to full.
    pub(crate) fn plan(&mut self, intermediate_required: bool) -> FramePlan {
        if std::mem::take(&mut self.force_full_next_frame) {
            self.damage.mark_full_repaint();
            tracing::trace!("promoting this frame to a full repaint");
        }
        plan_frame(
            &self.damage,
            self.retained.is_valid(),
            intermediate_required,
        )
    }

    /// Reconstruct every backdrop input before it reads the candidate target.
    /// Retained pixels outside damage contain the previous frame's *final*
    /// composition, including later siblings, not the current ordered backdrop.
    pub(crate) fn include_backdrop_dependencies(
        &mut self,
        scene: &flui_layer::Scene,
        size: (u32, u32),
    ) {
        let Some(mut damage) = self.damage.damage_rect() else {
            return;
        };
        let viewport = Rect::from_xywh(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let tree = scene.tree();
        let mut stack = vec![(scene.root(), glam::DMat4::IDENTITY)];
        let mut footprints = Vec::new();
        while let Some((id, parent)) = stack.pop() {
            let Some(node) = tree.get(id) else {
                continue;
            };
            let layer = node.layer();
            let transform = if let flui_layer::Layer::Transform(layer) = layer {
                parent * glam::DMat4::from_cols_array(&layer.transform().m)
            } else {
                let offset = if matches!(layer, flui_layer::Layer::Follower(_)) {
                    let Some(offset) = flui_layer::resolve_follower_offset(tree, id) else {
                        continue;
                    };
                    offset
                } else {
                    layer.local_translation()
                };
                parent * glam::DMat4::from_translation(glam::DVec3::new(offset.dx, offset.dy, 0.0))
            };
            if let flui_layer::Layer::BackdropFilter(layer) = layer {
                let Ok(affine) = crate::clip_geometry::ValidatedAffine::new(transform) else {
                    self.damage.mark_full_repaint();
                    return;
                };
                let Ok(output) = affine.map_bounds(layer.bounds()) else {
                    self.damage.mark_full_repaint();
                    return;
                };
                if let Some(output) = output.and_then(|bounds| bounds.intersect(&viewport)) {
                    let flui_painting::paint::ImageFilter::Blur { sigma_x, sigma_y } =
                        layer.filter()
                    else {
                        self.damage.mark_full_repaint();
                        return;
                    };
                    // Frobenius norm bounds each transformed axis. Twice sigma
                    // bounds the implemented ceil(sqrt(3)*sigma) kernel, with
                    // another pixel for f32 conversion and outward rounding.
                    // Ignore clips/isolation here: extra repaint is safe, missing
                    // input is not. Replay still admits its exact read footprint.
                    let scale = transform.x_axis.truncate().length_squared()
                        + transform.y_axis.truncate().length_squared();
                    let radius = (2.0 * sigma_x.max(*sigma_y) * scale.sqrt()).ceil() + 1.0;
                    if !radius.is_finite() || *sigma_x < 0.0 || *sigma_y < 0.0 {
                        self.damage.mark_full_repaint();
                        return;
                    }
                    if let Some(input) = output.expand(radius).intersect(&viewport) {
                        footprints.push((output, input));
                    }
                }
            }
            stack.extend(node.children().iter().map(|&child| (child, transform)));
        }
        // Growing one read region can expose a second filter. Reach a fixed
        // point before the partial clear and any recording, not next frame.
        for _ in 0..64 {
            let before = damage;
            for (output, input) in &footprints {
                if output.intersects(&damage) || input.intersects(&damage) {
                    damage = damage.union(input);
                }
            }
            if damage == before {
                self.damage.mark_dirty(damage);
                return;
            }
        }
        // An adversarial chain must not turn planning into quadratic work.
        // A full repaint reconstructs every dependency without further scans.
        self.damage.mark_full_repaint();
    }

    /// Runs `plan` (not [`FramePlan::Skip`]) against `surface`, whose texture
    /// is `size` in `format`.
    ///
    /// - [`FramePlan::Direct`] renders into the surface; the retained target
    ///   does not see the frame and stops being valid.
    /// - A retained plan renders into a separate candidate. Failure or unwind
    ///   preserves the committed pixels and forces a full retry.
    /// - Only a partial frame skips the full clear: it clears inside its
    ///   damage instead (`damage::begin_partial`), keeping the retained
    ///   pixels outside it.
    ///
    /// # Errors
    /// The content pass's failure.
    pub(crate) fn run(
        &mut self,
        plan: FramePlan,
        domain: &Arc<DeviceDomain>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        (surface_view, surface_texture): (&wgpu::TextureView, &wgpu::Texture),
        steps: &mut impl FrameSteps,
    ) -> EngineResult<()> {
        // A failure or unwind leaves damage owed and forces a deliberate full
        // retry; the previous committed allocation itself remains untouched.
        if matches!(plan, FramePlan::Skip) {
            return Ok(());
        }
        self.force_full_next_frame = true;
        let _submission_scope = domain.begin_frame_scope()?;
        let (retained, partial) = match plan {
            FramePlan::Skip => return Ok(()),
            FramePlan::Direct => {
                self.retained.invalidate();
                (None, None)
            }
            FramePlan::RetainedFull => (
                Some(self.retained.begin(domain, size, format, false)?),
                None,
            ),
            FramePlan::RetainedPartial(damage) => (
                Some(self.retained.begin(domain, size, format, true)?),
                Some(damage),
            ),
        };
        let (view, texture) = match retained.as_ref() {
            Some(candidate) => (candidate.view(), candidate.texture()),
            None => (surface_view, surface_texture),
        };
        if partial.is_none() {
            steps.clear(view)?;
        }
        let straddled = steps.content(view, texture, retained.is_some(), partial)?;
        if let Some(candidate) = retained {
            steps.blit(candidate.texture(), surface_view)?;
            self.retained.commit(candidate);
        }
        self.force_full_next_frame = straddled;
        Ok(())
    }

    /// The frame presented: the damage it covered is paid.
    pub(crate) fn presented(&mut self) {
        self.damage.reset();
    }

    /// Opens a frame that no damage producer accounted for (a scene rendered
    /// straight through `Renderer::render_scene` instead of a raster owner):
    /// it renders in full.
    pub(crate) fn begin_unmanaged(&mut self) {
        self.damage.mark_full_repaint();
    }

    /// Closes a frame opened by [`Self::begin_unmanaged`], whatever became of
    /// it. The frame showed a scene the producer never saw, so neither the
    /// retained target nor the producer's next diff describes the screen any
    /// more: the target is invalidated and the next frame renders in full,
    /// even one the producer found unchanged.
    pub(crate) fn end_unmanaged(&mut self) {
        self.retained.invalidate();
        self.force_full_next_frame = true;
    }

    /// The retained target's texture, when one is allocated.
    #[cfg(test)]
    pub(crate) fn retained_texture(&self) -> Option<&wgpu::Texture> {
        self.retained.texture()
    }
}
