use flui_rendering::pipeline::PipelinePhase;

struct CustomPhase;

impl PipelinePhase for CustomPhase {
    const NAME: &'static str = "Custom";
}

fn main() {}
