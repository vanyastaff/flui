use flui_interaction::{gesture::{GestureArena, ScaleGestureRecognizer, ScaleStartMode}};

fn main() {
    let _recognizer = ScaleGestureRecognizer::builder(GestureArena::new())
        .start_mode(ScaleStartMode::PanOrScale)
        .on_end(|details| {
            let _: flui_interaction::gesture::Velocity = details.focal_velocity;
            let _: f64 = details.velocity;
        })
        .build();
    let _default_two_contact_contract = ScaleStartMode::Scale;
}
