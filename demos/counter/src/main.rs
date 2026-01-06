//! Multi-Animation Demo for FLUI
//!
//! Demonstrates multiple animation types side by side:
//! - FadeTransition (red) - fades in/out
//! - ScaleTransition (green) - grows/shrinks
//! - RotationTransition (blue) - rotates continuously

use flui_animation::{AnimationController, Scheduler};
use flui_app::{AppBinding, AppConfig, BuildContext, StatelessView, View};
use flui_foundation::Listenable;
use flui_widgets::{Center, ColoredBox, FadeTransition, RotationTransition, Row, ScaleTransition};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
struct MultiAnimationDemo {
    fade_controller: AnimationController,
    scale_controller: AnimationController,
    rotation_controller: AnimationController,
}

impl MultiAnimationDemo {
    fn new() -> Self {
        let scheduler = Scheduler::arc_instance();

        let fade_controller =
            AnimationController::new(Duration::from_millis(1500), scheduler.clone());
        let scale_controller =
            AnimationController::new(Duration::from_millis(1800), scheduler.clone());
        let rotation_controller =
            AnimationController::new(Duration::from_millis(2000), scheduler.clone());

        let listener = Arc::new(|| {
            let binding = AppBinding::instance();
            binding.rebuild_root();
            binding.request_redraw();
        });

        fade_controller.add_listener(listener.clone());
        scale_controller.add_listener(listener.clone());
        rotation_controller.add_listener(listener.clone());

        fade_controller.repeat(true).expect("fade");
        scale_controller.repeat(true).expect("scale");
        rotation_controller.repeat(false).expect("rotation");

        Self {
            fade_controller,
            scale_controller,
            rotation_controller,
        }
    }
}

impl std::fmt::Debug for MultiAnimationDemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiAnimationDemo").finish()
    }
}

impl StatelessView for MultiAnimationDemo {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(
            Center::new().child(
                Row::new()
                    .spacing(40.0)
                    .child(
                        FadeTransition::new(self.fade_controller.clone())
                            .child(ColoredBox::red(100.0, 100.0)),
                    )
                    .child(
                        ScaleTransition::new(self.scale_controller.clone())
                            .child(ColoredBox::green(100.0, 100.0)),
                    )
                    .child(
                        RotationTransition::new(self.rotation_controller.clone())
                            .child(ColoredBox::blue(100.0, 100.0)),
                    ),
            ),
        )
    }
}

impl View for MultiAnimationDemo {
    fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
        Box::new(flui_view::StatelessElement::new(self))
    }
}

fn main() {
    println!("=== FLUI Multi-Animation Demo ===");
    println!("Red: Fade | Green: Scale | Blue: Rotation");

    let config = AppConfig::new()
        .with_title("FLUI Multi-Animation Demo")
        .with_size(800, 600);

    flui_app::run_app_with_config(MultiAnimationDemo::new(), config);
}
