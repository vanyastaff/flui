use std::rc::Rc;
use std::time::Duration;

use flui::animation::{Animation, AnimationController};

fn main() {
    let source = AnimationController::builder(Duration::from_secs(1)).build();
    let other = AnimationController::builder(Duration::from_secs(1)).build();
    let _subscription = source.subscribe_status(Rc::new(|_| {}));
    let token = source.add_status_listener(Rc::new(|_| {}));
    other.remove_status_listener(token);
}
