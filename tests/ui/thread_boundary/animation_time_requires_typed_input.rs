use flui::animation::{AnimationController, Vsync};

fn main() {
    let controller = AnimationController::builder(std::time::Duration::from_secs(1)).build();
    controller.tick_at(f64::NAN);
    Vsync::new().tick_all(f64::NAN);
}
