use flui_app::run_app_element;
use flui_view::RenderViewExt;
use flui_widgets::{Padding, Text};

fn main() {
    run_app_element(Padding::all(32.0).with_child(Text::headline("Hello, FLUI!").leaf()));
}
