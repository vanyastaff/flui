//! RenderTexture — draws an external texture into its whole box.

use flui_foundation::Diagnosticable;
use flui_foundation::Leaf;
use flui_foundation::geometry::{Rect, Size};
use flui_painting::paint::{FilterQuality, TextureId};
use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// Draws an external texture's current frame over its whole box.
///
/// The box takes the largest size the constraints allow (the smallest on an
/// unbounded axis): a texture has no intrinsic size of its own. The
/// `revision` is the number of frames the texture has received; a new one
/// repaints, so the frame the backend just uploaded reaches the screen even
/// when nothing else changed.
#[derive(Debug, Clone)]
pub struct RenderTexture {
    texture: TextureId,
    filter_quality: FilterQuality,
    revision: u64,
}

impl RenderTexture {
    /// Draws `texture`, sampled with `filter_quality`.
    pub fn new(texture: TextureId, filter_quality: FilterQuality) -> Self {
        Self {
            texture,
            filter_quality,
            revision: 0,
        }
    }

    /// The texture drawn.
    pub fn texture(&self) -> TextureId {
        self.texture
    }

    /// Draws another texture. Paint-only.
    pub fn set_texture(&mut self, texture: TextureId) -> RenderUpdateImpact {
        if self.texture == texture {
            return RenderUpdateImpact::NONE;
        }
        self.texture = texture;
        RenderUpdateImpact::PAINT
    }

    /// Changes how the texture is sampled. Paint-only.
    pub fn set_filter_quality(&mut self, filter_quality: FilterQuality) -> RenderUpdateImpact {
        if self.filter_quality == filter_quality {
            return RenderUpdateImpact::NONE;
        }
        self.filter_quality = filter_quality;
        RenderUpdateImpact::PAINT
    }

    /// Records that the texture received frame number `revision`; a new
    /// revision repaints.
    pub fn set_revision(&mut self, revision: u64) -> RenderUpdateImpact {
        if self.revision == revision {
            return RenderUpdateImpact::NONE;
        }
        self.revision = revision;
        RenderUpdateImpact::PAINT
    }

    fn size_for(constraints: &BoxConstraints) -> Size {
        let biggest = constraints.biggest();
        let smallest = constraints.smallest();
        Size::new(
            if biggest.width.is_finite() {
                biggest.width
            } else {
                smallest.width
            },
            if biggest.height.is_finite() {
                biggest.height
            } else {
                smallest.height
            },
        )
    }
}

impl Diagnosticable for RenderTexture {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add("texture", format!("{:?}", self.texture));
        properties.add("revision", self.revision.to_string());
    }
}

impl RenderBox for RenderTexture {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Self::size_for(ctx.constraints()))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Self::size_for(&constraints))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        let size = ctx.size();
        ctx.canvas().draw_texture(
            self.texture,
            Rect::from_xywh(0.0, 0.0, size.width, size.height),
            None,
            self.filter_quality,
            1.0,
        );
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        ctx.is_within_own_size()
    }
}
