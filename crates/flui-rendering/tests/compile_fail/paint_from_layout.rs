use flui_rendering::{PipelineOwner, TextContextHandle};

fn main() {
    let mut owner = PipelineOwner::new(TextContextHandle::standalone());
    let mut layout = owner.layout_pass();
    layout.run_paint();
}
