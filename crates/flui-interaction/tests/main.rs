//! flui-interaction's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "headless_long_press.rs"]
mod headless_long_press;

#[path = "interaction_lane.rs"]
mod interaction_lane;
