//! What the input position of the winit host's table answers; the window's is
//! the window feature's (`window::winit`).

use crate::host;
use crate::owner::winit::WinitLoop;

impl host::Input<WinitLoop> for WinitLoop {
    fn keyboard_layout(_: &WinitLoop) -> String {
        // winit does not report the active keyboard layout: it is unknown.
        String::new()
    }

    fn on_keyboard_layout_change(_: &WinitLoop, callback: Box<dyn FnMut() + Send>) {
        // No layout is read, so no change is ever reported.
        drop(callback);
    }
}
