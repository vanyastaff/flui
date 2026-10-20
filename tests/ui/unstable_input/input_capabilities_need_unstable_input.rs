// Without the `unstable-input` feature the facade keeps input availability
// private (ADR-0213 §5); with it this compiles.
use flui::platform::input_capabilities::InputCapabilities;

fn main() {
    let _ = std::any::type_name::<InputCapabilities>();
}
