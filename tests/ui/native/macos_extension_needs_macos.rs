// `flui::native::macos` exists only on macOS; elsewhere, with the `native`
// feature, the path does not resolve.
use flui::native::macos::AppKitWindow;

fn main() {
    let _ = std::any::type_name::<AppKitWindow<'static>>();
}
