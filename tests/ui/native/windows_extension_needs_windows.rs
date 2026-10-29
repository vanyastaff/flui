// `flui::native::windows` exists only on Windows; elsewhere, with the
// `native` feature, the path does not resolve.
use flui::native::windows::Win32Window;

fn main() {
    let _ = std::any::type_name::<Win32Window<'static>>();
}
