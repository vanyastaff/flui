use flui_foundation::geometry::{Matrix4, Size};
use flui_rendering::{
    RenderResult,
    context::{BoxIntrinsicsCtx, FragmentRecorder},
    protocol::{BoxProtocol, Protocol, ProtocolPosition},
    storage::IntrinsicDimension,
    traits::{HitTestOutcome, RenderObject},
};

#[derive(Debug)]
struct RawConsumer;

impl flui_foundation::Diagnosticable for RawConsumer {}

impl RenderObject<BoxProtocol> for RawConsumer {
    fn perform_layout_raw(
        &mut self,
        _ctx: &mut <BoxProtocol as Protocol>::LayoutCtxErased<'_>,
    ) -> RenderResult<Size> {
        Ok(Size::ZERO)
    }

    fn paint_raw(&self, _recorder: &mut FragmentRecorder, _child_count: usize, _size: Size) {}

    fn hit_test_raw(
        &self,
        _position: ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }

    fn intrinsic_raw(
        &self,
        dimension: IntrinsicDimension,
        extent: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        let mut text = ctx.text()?;
        let width = ctx.child_intrinsic(0, dimension, extent)?;
        let _measurement = text.measurement();
        Ok(width)
    }
}

fn main() {}
