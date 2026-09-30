//! The frame protocol the windowed renderer and the headless retained
//! capture share (ADR-0087 §4): the damage owed, the retained target that
//! holds the last frame, and the sequence that turns a [`FramePlan`](crate::damage::FramePlan) into
//! GPU work — pick the target, clear it unless the frame is partial, record
//! the content, blit a retained frame to the surface, commit.
//!
//! Only the steps that genuinely differ between a swapchain and a readback
//! texture ([`FrameSteps`](crate::frame_protocol::FrameSteps)) live with each caller; everything a readback test
//! pins about partial frames runs through this one implementation.

use flui_foundation::geometry::Rect;

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
    fn clear(&mut self, view: &wgpu::TextureView);

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
    fn blit(&mut self, retained: &wgpu::Texture, surface: &wgpu::TextureView);
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

    /// Runs `plan` (not [`FramePlan::Skip`]) against `surface`, whose texture
    /// is `size` in `format`.
    ///
    /// - [`FramePlan::Direct`] renders into the surface; the retained target
    ///   does not see the frame and stops being valid.
    /// - A retained plan renders into the target, which stays invalid from
    ///   `begin` until the blit has been recorded, so a frame that fails or
    ///   unwinds in between leaves the next partial frame rendering in full.
    /// - Only a partial frame skips the full clear: it clears inside its
    ///   damage instead (`damage::begin_partial`), keeping the retained
    ///   pixels outside it.
    ///
    /// # Errors
    /// The content pass's failure.
    pub(crate) fn run(
        &mut self,
        plan: FramePlan,
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        (surface_view, surface_texture): (&wgpu::TextureView, &wgpu::Texture),
        steps: &mut impl FrameSteps,
    ) -> EngineResult<()> {
        let (retained, partial) = match plan {
            FramePlan::Skip => return Ok(()),
            FramePlan::Direct => {
                self.retained.invalidate();
                (None, None)
            }
            FramePlan::RetainedFull => (Some(self.retained.begin(device, size, format)), None),
            FramePlan::RetainedPartial(damage) => (
                Some(self.retained.begin(device, size, format)),
                Some(damage),
            ),
        };
        let (view, texture) = match retained.as_ref() {
            Some((texture, view)) => (view, texture),
            None => (surface_view, surface_texture),
        };
        if partial.is_none() {
            steps.clear(view);
        }
        let straddled = steps.content(view, texture, retained.is_some(), partial)?;
        self.force_full_next_frame |= straddled;
        if let Some((texture, _)) = retained.as_ref() {
            steps.blit(texture, surface_view);
            self.retained.commit();
        }
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
