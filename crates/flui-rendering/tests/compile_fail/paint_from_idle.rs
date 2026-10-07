use flui_rendering::{PipelineOwner, TextContextHandle};

fn main() {
    let mut owner = PipelineOwner::new(TextContextHandle::standalone());
    owner.run_paint();
}
