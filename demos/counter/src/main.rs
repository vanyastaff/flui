use flui_app::run_app_element;
use flui_widgets::Text;

fn main() {
    eprintln!("=== Starting Text Demo ===");
    run_app_element(Text::headline("Hello, FLUI!"));
}
