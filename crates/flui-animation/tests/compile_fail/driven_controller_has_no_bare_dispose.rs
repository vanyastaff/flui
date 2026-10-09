use std::time::Duration;

use flui::animation::{AnimationController, Vsync};

fn main() {
    let registry = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let observer = owner.controller().clone();
    observer.dispose();
}
