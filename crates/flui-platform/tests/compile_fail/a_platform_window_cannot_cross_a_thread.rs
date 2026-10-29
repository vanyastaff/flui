use std::rc::Rc;
use std::sync::Arc;

use flui_platform::PlatformWindow;

fn send_rc(window: Rc<dyn PlatformWindow>) {
    std::thread::spawn(move || window.request_redraw());
}

fn send_arc(window: Arc<dyn PlatformWindow>) {
    std::thread::spawn(move || window.request_redraw());
}

fn send_borrow(window: &'static dyn PlatformWindow) {
    std::thread::spawn(move || window.request_redraw());
}

fn main() {}
