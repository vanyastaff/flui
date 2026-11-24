//! The environment variables `flui run` sets for the application it launches.
//!
//! `flui-cli` keeps its own copy of these names, so that it does not link the
//! framework; its test `env_names_match_the_runtime` holds the two in step.

/// Path of the worker `cdylib` a `--hot-reload` host loads
/// (`my_app_logic.dll`).
pub const WORKER_PLUGIN: &str = "FLUI_WORKER_PLUGIN";

/// Path of a scene or app plugin shared library for in-process reload.
///
/// Example: `FLUI_SCENE_PLUGIN=target/debug/libflui_scene.so`
pub const SCENE_PLUGIN: &str = "FLUI_SCENE_PLUGIN";

/// `"1"` when `flui run` launched the application with hot reload on.
pub const HOT_RELOAD: &str = "FLUI_HOT_RELOAD";
