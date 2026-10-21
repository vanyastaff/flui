use flui_interaction::{GestureArena, ScaleGestureRecognizer, gesture::ScaleStartMode};

fn main() {
    let _recognizer = ScaleGestureRecognizer::builder(GestureArena::new())
        .start_mode(ScaleStartMode::PanOrScale)
        .on_end(|details| {
            let _: flui_interaction::Velocity = details.focal_velocity;
            let _: f64 = details.velocity;
        })
        .build();
    let _default_two_contact_contract = ScaleStartMode::Scale;
}
