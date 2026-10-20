// Without the `unstable-input` feature the facade keeps the second haptic
// tier private (ADR-0210 §9); with it this compiles.
use flui::platform::haptics::HapticPattern;

fn main() {
    let _ = std::any::type_name::<HapticPattern>();
}
