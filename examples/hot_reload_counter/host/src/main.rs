//! Host binary for the Flutter-parity hot-reload counter demo.
//!
//! ```bash
//! cd examples/hot_reload_counter
//! flui run
//! ```
//!
//! Edit `INCREMENT_LABEL` in `logic/src/lib.rs` and save — CLI rebuilds the worker;
//! the host hot-reloads in-process (counter value preserved).
//!
//! With `FLUI_WORKER_PLUGIN` unset, defaults to `target/debug/counter_logic.{dll,so,dylib}`.
//! Edit `logic/src/lib.rs`, rebuild the logic crate, and the counter value is preserved.

use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

use flui_app::{AppConfig, run_app_with_config};
use flui_hot_reload::engine::env;
use hot_reload_counter_types::{CounterApp, CounterShell};

fn default_worker_path() -> std::path::PathBuf {
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let mut path = std::path::PathBuf::from("target");
    path.push(&profile);

    #[cfg(windows)]
    {
        path.push("counter_logic.dll");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        path.push("libcounter_logic.so");
    }
    #[cfg(target_os = "macos")]
    {
        path.push("libcounter_logic.dylib");
    }

    path
}

fn main() {
    let worker_path = std::env::var(env::WORKER_PLUGIN)
        .map_or_else(|_| default_worker_path(), std::path::PathBuf::from);

    tracing::info!(
        worker = %worker_path.display(),
        "Hot-reload counter host — build logic crate and edit INCREMENT_LABEL to test"
    );

    let count = Arc::new(AtomicI32::new(0));
    let root = CounterShell {
        app: CounterApp {
            initial: 0,
            count: Arc::clone(&count),
        },
    };

    let config = AppConfig::new()
        .with_title("FLUI Hot-Reload Counter")
        .with_size(480, 320)
        .with_worker_plugin_path(worker_path);

    run_app_with_config(root, config);

    tracing::info!(
        final_count = count.load(Ordering::Relaxed),
        "Application exited"
    );
}
