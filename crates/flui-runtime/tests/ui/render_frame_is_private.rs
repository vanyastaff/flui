use flui_runtime::sink::FrameSink;
use flui_runtime::ui_runtime::UiRuntime;

fn frame(realm: &mut UiRuntime, sink: &mut dyn FrameSink) -> bool {
    realm.render_frame(sink)
}

fn main() {}
