use flui_widgets::{OverlayEntry, OverlayHandle};

fn requires_send<T: Send>() {}
fn requires_sync<T: Sync>() {}

fn main() {
    requires_send::<OverlayHandle>();
    requires_sync::<OverlayHandle>();
    requires_send::<OverlayEntry>();
    requires_sync::<OverlayEntry>();
}
