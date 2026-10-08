use flui_sdk::view::View;
use flui_view::__runtime::BindingRuntime;

fn runtime<T: BindingRuntime>(_binding: &T) {}
fn view<T: View>(_view: &T) {}

fn main() {}
