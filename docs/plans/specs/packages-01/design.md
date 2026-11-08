# Design: bounded facade application surface

The facade defines a documented `app` module instead of aliasing `flui_app`. It contains `AppHandle`, `ComputeJob`, `DeterministicExecutors`, `HostComputePool`, `HostExecutors`, `HostIoPool`, `IoFuture` and `SpawnError`, with the existing target gates. `AppConfig`, launch types, window creation and `run_app` stay at the root and in the design-neutral prelude. The hot-reload template and facade fixtures use those paths.

`flui_app::native::android` owns the Android-only re-exports of `AndroidApp` and `run_app_android`; the facade re-exports that module as `flui::native::android`. The old root `flui::android_activity` and `flui::run_app_android` disappear. Empty, basic and counter templates add a target-specific Android `flui` dependency with `native` and call the new path.

The root manifest opts into the existing API-closure gate with all-feature Linux, Windows, macOS, iOS, Android and wasm32 builds. AccessKit's already documented testing re-export is the only anticipated temporary exception; Android receives none. Send-position inventories are regenerated and inspected for removals only.

The change also rewrites documents that teach the whole-alias paths, removes the Android gap, and records both removals in a changelog fragment. No compatibility alias or parallel facade module remains.
