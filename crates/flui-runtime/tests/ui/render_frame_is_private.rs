use flui_runtime::sink::FrameSink;
use flui_runtime::ui_realm::UiRealm;

fn frame(realm: &mut UiRealm, sink: &mut dyn FrameSink) -> bool {
    realm.render_frame(sink)
}

fn main() {}
