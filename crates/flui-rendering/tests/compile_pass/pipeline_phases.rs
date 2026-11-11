use flui_rendering::{PipelineOwner, TextContextHandle, testing::SceneHost};

fn main() {
    let mut scene = SceneHost::new();
    let owner = PipelineOwner::new(TextContextHandle::standalone());
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing");
    let mut owner = owner.into_paint();
    let (painted, _tree) = scene.drive(&mut owner, |owner, writer| owner.run_paint(writer));
    painted.expect("paint");
    let mut owner = owner.into_semantics();
    owner.run_semantics().expect("semantics");
    let owner = owner.finish();
    let (_owner, result) = scene.run_frame(owner);
    result.expect("frame");
}
