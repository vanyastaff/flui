use flui_rendering::binding::RendererBinding;
use flui_runtime::pump::SampledClock;
use flui_runtime::renderer_binding::RenderingBinding;
use flui_runtime::sink::FrameSink;
use flui_runtime::ui_realm::UiRealm;

fn frame(realm: &mut UiRealm, sink: &mut dyn FrameSink) -> bool {
    realm
        .pump(&mut SampledClock(web_time::Instant::now()), sink)
        .presented()
}

fn main() {
    let binding = RenderingBinding::new(flui_rendering::TextContextHandle::standalone());
    let pipeline = binding.root_pipeline_owner().clone();
    let read = move || {
        pipeline.with(|owner| owner.root_id());
    };
    read();
    let _ = frame;
}
