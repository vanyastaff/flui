//! The layer-tree state hand-off: clips, transforms, opacity, and filters.
//!
//! The layer walk calls these to mirror a layer tree's nesting onto the
//! backend's state stack; [`CommandRenderer`](crate::command_renderer::CommandRenderer)
//! is the sibling trait that carries the per-`DrawCommand` vocabulary.
//!
//! These methods used to live on `CommandRenderer` alongside its per-command
//! visitor methods. Splitting them out keeps the two responsibilities legible,
//! even though every implementor in this crate implements both: a backend that
//! cannot mirror the clip/transform nesting cannot render a real tree.

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, RRect, RSuperellipse, Rect},
    painting::Path,
};

/// Compositor hand-off interface for the flui-layer clip/transform/effect
/// stacks.
///
/// These methods used to live on [`CommandRenderer`] alongside its 34
/// per-command visitor methods. They were split out into this dedicated
/// trait because:
///
/// - The visitor methods (render_rect / render_text / ...) are the
///   `DrawCommand` dispatch contract every backend implements -- a
///   software fallback, a debug recorder, a Skia backend, all
///   conceptually answer "given this draw command, produce these
///   pixels".
/// - The push/pop methods are flui-layer's clip-stack hand-off
///   mechanism. They are framework-internal: the layer tree (see
///   `crates/flui-engine/src/wgpu/layer_render.rs`) walks layers
///   recursively, calling `push_clip_*` / `pop_clip` / `push_opacity`
///   / etc. to mirror the layer-tree's nesting onto the painter's
///   internal state stack.
///
/// Implemented by every backend that renders a real layer tree, alongside
/// [`CommandRenderer`]: the clip/transform/effect nesting has to mirror the
/// tree or the rendered output is wrong. A recorder that only counts
/// commands can skip this half, which is what the split buys.
pub trait LayerStateStack {
    /// Push a rectangular clip onto the clip stack
    fn push_clip_rect(&mut self, rect: &Rect<Pixels>, clip_behavior: flui_types::painting::Clip);

    /// Push a rounded rectangular clip onto the clip stack
    fn push_clip_rrect(&mut self, rrect: &RRect, clip_behavior: flui_types::painting::Clip);

    /// Push an arbitrary path clip onto the clip stack
    fn push_clip_path(&mut self, path: &Path, clip_behavior: flui_types::painting::Clip);

    /// Push a rounded-superellipse (iOS squircle) clip onto the clip stack.
    ///
    /// Required rather than defaulted. The obvious default —
    /// approximating with the rounded rectangle that shares this shape's outer
    /// rect and radii — is not the conservative choice it reads as: that rrect
    /// is **inscribed** in the squircle, so it clips strictly MORE and silently
    /// discards corner content. (`a_clip_superellipse_layer_clips_to_the_squircle_
    /// not_its_bounding_rrect` measures the gap: 2.9 px at a 64 px shape.) A
    /// default forwarding to [`push_clip_path`](Self::push_clip_path) would be
    /// worse still on a backend where path clipping is a no-op — it would
    /// reinstate the silent no-op this method exists to remove (issue #921).
    ///
    /// The in-tree precedent for a degrading default on this trait has already
    /// misfired: `MockRenderer` never overrode
    /// [`push_opacity_blend`](Self::push_opacity_blend), so a blend-mode
    /// opacity is still recorded as a plain `push_opacity`. An implementor that
    /// cannot evaluate a squircle should choose its approximation deliberately
    /// and say so, which a compile error asks for and a default does not.
    fn push_clip_rsuperellipse(
        &mut self,
        rse: &RSuperellipse,
        clip_behavior: flui_types::painting::Clip,
    );

    /// Pop the most recent clip from the clip stack
    fn pop_clip(&mut self);

    /// Push a translation offset onto the transform stack
    fn push_offset(&mut self, offset: Offset<Pixels>);

    /// Push a full matrix transformation onto the transform stack
    fn push_transform(&mut self, transform: &Matrix4);

    /// Pop the most recent transform from the transform stack
    fn pop_transform(&mut self);

    /// Push an opacity value onto the effect stack
    fn push_opacity(&mut self, alpha: f32);

    /// Push an opacity layer with an explicit blend mode onto the effect stack.
    ///
    /// The default implementation forwards to [`push_opacity`](Self::push_opacity),
    /// which is correct for command-only backends that do not participate in the
    /// dst-read compositor path.  The `wgpu` backend overrides this to route
    /// advanced blend modes through `save_layer` with the blend propagated.
    fn push_opacity_blend(&mut self, alpha: f32, blend: flui_types::painting::BlendMode) {
        let _ = blend;
        self.push_opacity(alpha);
    }

    /// Pop the most recent opacity from the effect stack
    fn pop_opacity(&mut self);

    /// Push a color filter onto the effect stack.
    ///
    /// Accepts the full [`flui_types::painting::ColorFilter`] enum — `Matrix`,
    /// `Mode`, `LinearToSrgbGamma`, and `SrgbToLinearGamma` — so all engine
    /// GPU passes are reachable from a single trait method.
    fn push_color_filter(&mut self, filter: &flui_types::painting::ColorFilter);

    /// Pop the most recent color filter from the effect stack
    fn pop_color_filter(&mut self);

    /// Push an image filter onto the effect stack
    fn push_image_filter(&mut self, filter: &flui_painting::display_list::ImageFilter);

    /// Pop the most recent image filter from the effect stack
    fn pop_image_filter(&mut self);
}
