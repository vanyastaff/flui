use flui_platform::{PlatformWindow, WindowCommand};

fn show(window: &dyn PlatformWindow) {
    let _ = window.apply(WindowCommand::Show);
}

fn main() {}
