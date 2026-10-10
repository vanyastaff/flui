use flui_foundation::geometry::{Matrix4, Size};
use flui_rendering::{
    RenderResult,
    constraints::BoxConstraints,
    context::FragmentRecorder,
    protocol::{BoxProtocol, Protocol, ProtocolPosition},
    traits::{HitTestOutcome, RenderObject},
};

#[derive(Debug)]
struct RawConsumer;

impl flui_foundation::Diagnosticable for RawConsumer {}

impl RenderObject<BoxProtocol> for RawConsumer {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as Protocol>::LayoutCtxErased<'_>,
    ) -> RenderResult<Size> {
        let mut text = ctx.text()?;
        let size = ctx.layout_child(0, BoxConstraints::new(0.0, 100.0, 0.0, 100.0))?;
        let _measurement = text.measurement();
        Ok(size)
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
}

fn main() {}
