use flui_native::HostWindow;

fn read(window: &dyn HostWindow) {
    let _ = window.text_store_host(Default::default());
}

fn main() {}
