use flui_rendering::{PipelineOwner, TextContextHandle};

fn main() {
    let owner = PipelineOwner::new(TextContextHandle::standalone());
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("paint");
    let mut owner = owner.into_semantics();
    owner.run_semantics().expect("semantics");
    let owner = owner.finish();
    let (_owner, result) = owner.run_frame();
    result.expect("frame");
}
