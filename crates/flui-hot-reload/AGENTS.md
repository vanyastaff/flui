# AGENTS.md — flui-hot-reload

Hot-reload support for scene plugins via dynamic library loading (dlopen/LoadLibrary).

## What lives here

- **`scene_plugin!` macro** — generates `extern "C"` wrappers around a user's `fn(f32, f32) -> Scene` function for the plugin side
- **`ScenePlugin`** — host-side loader: loads a shared library, checks mtime for updates, reloads automatically
- **`dynlib` module** — cross-platform abstraction over Unix `dlopen`/`dlsym`/`dlclose` and Windows `LoadLibraryW`/`GetProcAddress`/`FreeLibrary`

## Key constraints

- **FFI pattern** — plugin passes opaque `Box::into_raw` pointer across FFI boundary. No serialization, no `#[repr(C)]` types needed. Host takes ownership back via `Box::from_raw`.
- **`app-plugin` feature** — gates `flui-view`, `flui-rendering`, `flui-types`, `parking_lot` deps for widget-based hot-reloadable plugins (not just scene-based).
- **Platform deps** — `windows 0.62` (Win32), `libc` (Unix), `android_log-sys` (Android logging).
- **Core dependency** — only `flui-layer` (always). Scene plugins build real `LayerTree` using normal FLUI APIs.

## Examples

- `examples/desktop_scene/` — hot-reloadable desktop scene plugin
- `examples/android_scene/` — Android scene plugin (cdylib)
- `examples/android_app/` — Widget-based hot-reloadable plugin (cdylib)
