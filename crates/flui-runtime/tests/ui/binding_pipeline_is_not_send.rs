use flui_rendering::binding::RendererBinding;
use flui_runtime::renderer_binding::RenderingBinding;

fn main() {
    let binding = RenderingBinding::new(flui_rendering::TextContextHandle::standalone());
    let pipeline = binding.root_pipeline_owner().clone();
    std::thread::spawn(move || {
        pipeline.with(|owner| owner.root_id());
    });
}
