use flui_rendering::{PipelineOwner, TextContextHandle};

fn main() {
    let owner = PipelineOwner::new(TextContextHandle::standalone());
    let mut owner = owner.into_layout();
    owner.run_paint();
}
