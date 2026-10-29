// Without the `native` feature the facade has no `flui::native` (ADR-0254 §8);
// with it this compiles.
use flui::native::HostWindow;

fn main() {
    let _ = std::any::type_name::<dyn HostWindow>();
}
