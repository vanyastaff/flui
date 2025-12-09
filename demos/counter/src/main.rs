use flui_app::run_app_element;
use flui_view::RenderViewExt;
use flui_widgets::Text;

fn main() {
    run_app_element(Text::headline("Hello, FLUI!").leaf());
}
