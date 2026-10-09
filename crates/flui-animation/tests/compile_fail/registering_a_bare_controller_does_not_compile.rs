use std::time::Duration;

use flui::animation::{AnimationController, Vsync};

fn main() {
    let registry = Vsync::new();
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let seat = registry.register(controller.clone());
    let _ = registry.try_register(&controller);
    registry.unregister(&seat);
}
