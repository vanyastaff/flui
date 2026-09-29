# flui-hot-reload

**`dlopen`-based hot-reload host for FLUI scenes** — iterate on desktop UI
without restarting the process.

`flui-hot-reload` loads scene plugins (cdylibs) at runtime via a
`HotReloadDriver`, watches for rebuilds, and swaps the plugin in place while
the host window and GPU context stay alive. See `examples/desktop_scene/` and
`examples/hot_reload_counter/` for the host/plugin pair layout.

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace — pre-release,
consumed by path (not published to crates.io).

## How it works

```text
host process (flui-app window + wgpu context)
    │ dlopen / dlclose on rebuild
    ▼
scene plugin (cdylib): exposes the scene entry points
    │ versioned ABI handshake guards mismatched host/plugin builds
    ▼
render pipeline continues with the fresh scene
```

Android hot-reload uses the same plugin contract (see the `examples/android_*`
crates, built via `cargo ndk`).

## Installing in a host

`flui-app` does not depend on this crate. A host installs one of its
`DevReloadHook`s (ADR-0094 §1) on its configuration, and the runner drives it:

```rust,ignore
use flui::hot_reload::{ScenePluginHook, WorkerReloadHook};

// Desktop host/worker split (`flui create --hot-reload`):
let config = AppConfig::new().with_dev_reload(WorkerReloadHook::new(worker_path));

// Android `flui run --scene`:
let path = ScenePluginHook::device_library_path(app.internal_data_path().as_deref());
let config = AppConfig::new().with_dev_reload(ScenePluginHook::new(path));
```

## Documentation

Every public item is documented (`#![deny(missing_docs)]`); build locally with
`cargo doc -p flui-hot-reload --open`.

## License

MIT OR Apache-2.0, per the workspace license.
