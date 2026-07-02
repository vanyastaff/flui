//! `Flow` widget smoke coverage over `RenderFlow`.

mod common;

use std::any::Any;
use std::sync::Arc;

use common::{lay_out, loose, offset, size};
use flui_rendering::constraints::BoxConstraints;
use flui_types::{Matrix4, Size};
use flui_widgets::row;
use flui_widgets::{Flow, FlowDelegate, FlowPaintingContext, SizedBox};

/// Places child `i` at `(i * step, 0)` via a paint-time transform — layout
/// itself always positions every `Flow` child at zero, so this delegate
/// exercises the transform, not the layout offset.
#[derive(Debug)]
struct StepDelegate {
    step: f32,
}

impl FlowDelegate for StepDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        BoxConstraints::loose(constraints.biggest())
    }

    fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
        for i in 0..context.child_count() {
            context.paint_child(i, Matrix4::translation(i as f32 * self.step, 0.0, 0.0));
        }
    }

    fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn step_delegate(step: f32) -> Arc<dyn FlowDelegate> {
    Arc::new(StepDelegate { step })
}

#[test]
fn flow_mounts_render_flow_and_sizes_via_the_delegate() {
    let laid = lay_out(
        Flow::new(
            step_delegate(30.0),
            row![SizedBox::new(20.0, 20.0), SizedBox::new(20.0, 20.0)],
        ),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderFlow"), root);
    // StepDelegate::get_size returns `constraints.biggest()` — Flow's own
    // size comes from the delegate, not from shrink-wrapping its children.
    assert_eq!(laid.size(root), size(200.0, 200.0));
}

#[test]
fn flow_lays_out_every_child_at_zero_offset_regardless_of_paint_transform() {
    let laid = lay_out(
        Flow::new(
            step_delegate(30.0),
            row![SizedBox::new(20.0, 20.0), SizedBox::new(20.0, 20.0)],
        ),
        loose(200.0),
    );

    let root = laid.root();
    let first = laid.child(root, 0);
    let second = laid.child(root, 1);
    assert_eq!(laid.size(first), size(20.0, 20.0));
    assert_eq!(laid.size(second), size(20.0, 20.0));
    // The whole point of Flow: layout never positions children — only the
    // delegate's paint-time transform does.
    assert_eq!(laid.offset(first), offset(0.0, 0.0));
    assert_eq!(laid.offset(second), offset(0.0, 0.0));
}

#[test]
fn flow_childless_sizes_via_delegate_alone() {
    let laid = lay_out(
        Flow::new(step_delegate(10.0), Vec::<flui_view::BoxedView>::new()),
        loose(150.0),
    );

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderFlow"), root);
    assert_eq!(laid.size(root), size(150.0, 150.0));
}
