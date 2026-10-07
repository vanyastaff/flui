use flui::view::View;
use flui_view::__runtime::BindingRuntime;

fn composition_root<T: BindingRuntime>(_binding: &T) {}
fn app_view<V: View>(_view: &V) {}

fn main() {}
