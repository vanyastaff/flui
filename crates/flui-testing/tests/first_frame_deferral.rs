//! Recovery of the public first-frame presentation gate.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_foundation::geometry::Size;
use flui_objects::RenderColoredBox;
use flui_rendering::binding::RendererBinding;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner, TextContextHandle};
use flui_runtime::renderer_binding::RenderingBinding;
use flui_scheduler::UpdateScheduler;

pub(crate) fn unmatched_first_frame_release_preserves_the_next_deferral() {
    let pipeline = PipelineCell::new(PipelineOwner::new(TextContextHandle::standalone()));
    let root = pipeline.with_mut(|owner| {
        let root = owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(
            RenderColoredBox::red(40.0, 40.0),
        ));
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));
        root
    });
    let scheduler = UpdateScheduler::new();
    let binding = RenderingBinding::new_with_pipeline(pipeline, &scheduler);
    for _ in 0..2 {
        assert!(catch_unwind(AssertUnwindSafe(|| binding.allow_first_frame())).is_err());
    }
    binding.defer_first_frame();
    assert!(
        binding.draw_frame().is_none(),
        "the new deferral withholds paint"
    );
    binding.allow_first_frame();
    binding
        .root_pipeline_owner()
        .with_mut(|owner| owner.mark_needs_layout(root));
    let painted = binding
        .draw_frame()
        .expect("the matched release permits paint");
    assert!(
        painted.len() > 1,
        "the frame carries the colored render object"
    );
    // The refusal remains harmless after a successful frame too.
    assert!(catch_unwind(AssertUnwindSafe(|| binding.allow_first_frame())).is_err());
    assert!(binding.send_frames_to_engine());
}
