use flui_rendering::binding::RendererBinding;
use flui_runtime::renderer_binding::RenderingBinding;

fn require_send<T: Send>(_: T) {}

fn main() {
    let binding = RenderingBinding::new(flui_rendering::TextContextHandle::standalone());
    let pipeline = binding.root_pipeline_owner().clone();
    require_send(pipeline);
}
