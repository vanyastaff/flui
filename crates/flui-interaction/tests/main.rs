//! flui-interaction's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "headless_long_press.rs"]
mod headless_long_press;

#[path = "multi_tap.rs"]
mod multi_tap;

#[path = "interaction_lane.rs"]
mod interaction_lane;

#[path = "text_input_retirement.rs"]
mod text_input_retirement;

#[path = "hit_test_transform.rs"]
mod hit_test_transform;

#[path = "focus_retention.rs"]
mod focus_retention;

#[path = "text_store_host.rs"]
mod text_store_host;

mod button_delivery;
#[path = "gesture_lifecycle.rs"]
mod gesture_lifecycle;
#[path = "mouse_tracking.rs"]
mod mouse_tracking;
#[path = "multi_pointer_recognizers.rs"]
mod multi_pointer_recognizers;
mod pointer_identity;
#[path = "pointer_source.rs"]
mod pointer_source;
#[path = "recognizer_api.rs"]
mod recognizer_api;
#[path = "recognizer_lifecycle.rs"]
mod recognizer_lifecycle;
#[path = "velocity_and_resampling.rs"]
mod velocity_and_resampling;
