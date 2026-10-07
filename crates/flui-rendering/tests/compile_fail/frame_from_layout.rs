use flui_rendering::{PipelineOwner, TextContextHandle};

fn main() {
    let owner = PipelineOwner::new(TextContextHandle::standalone());
    let owner = owner.into_layout();
    let _ = owner.run_frame();
}
