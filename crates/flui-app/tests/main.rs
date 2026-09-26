//! flui-app's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "data_transfer_transport.rs"]
mod data_transfer_transport;

#[path = "execution_public_paths.rs"]
mod execution_public_paths;

#[path = "runner_frame_ordering.rs"]
mod runner_frame_ordering;
