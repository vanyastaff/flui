use flui_app::run_app_element;
use flui_widgets::{Padding, Text};

fn main() {
    run_app_element(Padding::all(32.0).child(Text::headline("Hello, FLUI!")));
}
