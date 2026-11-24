use flui_layer::SceneOwner;
use flui_rendering::{PipelineOwner, TextContextHandle};

fn main() {
    let mut scene = SceneOwner::new();
    let mut owner = PipelineOwner::new(TextContextHandle::standalone());
    let layout = owner.layout_pass();
    let _ = owner.run_frame(&mut scene.writer());
    drop(layout);
}
