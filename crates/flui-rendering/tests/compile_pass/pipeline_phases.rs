use flui_rendering::{PipelineOwner, TextContextHandle, testing::SceneHost};

fn main() {
    let mut scene = SceneHost::new();
    let mut owner = PipelineOwner::new(TextContextHandle::standalone());
    let mut layout = owner.layout_pass();
    layout.run_layout().expect("layout");
    let mut compositing = layout.into_compositing();
    compositing.run_compositing();
    let (painted, _tree) = scene.drive(&mut owner, |owner, writer| {
        owner.layout_pass().into_compositing().into_paint().run_paint(writer)
    });
    painted.expect("paint");
    let mut semantics = owner.layout_pass().into_compositing().into_paint().into_semantics();
    semantics.run_semantics().expect("semantics");
    scene.run_frame(&mut owner).expect("frame");
}
