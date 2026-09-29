# Hot-reload plugin boundary

Hot reload is a development-only Rust payload ABI. Host and plugin builds must
agree on layout through the existing ABI-token handshake and keep the plugin
image mapped while plugin-backed values remain alive. This is not a stable C
representation of a scene.

## Mapping decisions

### Typed scene factories and explicit ownership transfer

The plugin macros export fixed C symbol families once per image. Factories return
the canonical `Scene`, publicly reexported from its owning layer crate. Macro
expansions resolve it through `$crate`, so application authors can invoke either
macro through `flui::hot_reload` or a renamed facade dependency. No consumer
layer dependency is necessary.

Every returned allocation has exactly two mutually exclusive consumption paths:

- Keep the original initialized scene in its box, then call that image's
  `flui_scene_drop` or `flui_app_drop` once.
- Move the scene out with `ptr::read` exactly once, then call that image's
  `flui_scene_free` or `flui_app_free` once to deallocate the emptied box.

The host never reconstructs a `Box` to free plugin memory. Free uses
`MaybeUninit<Scene>`, which preserves allocation size/alignment without dropping
the moved value. Both teardown functions are `unsafe extern "C"`: the caller
must supply the live allocation, preserve unique ownership, and choose the
matching consumption path. Null is a no-op. The C ABI and symbol names remain
unchanged; Rust callers now must acknowledge their obligations explicitly.

Keep the image loaded until both its allocation and every retained plugin-backed
payload have been destroyed, including payloads moved out or cloned from a
scene. Destroying the allocation alone does not discharge this obligation.

This follows Rust's ownership rules, rather than a Flutter lifecycle contract:
[`Box::from_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.from_raw)
requires the original allocation layout and compatible allocator;
[`ptr::read`](https://doc.rust-lang.org/std/ptr/fn.read.html) moves ownership without
changing the allocation; [`MaybeUninit`](https://doc.rust-lang.org/std/mem/union.MaybeUninit.html)
has its payload's layout and omits its destructor. The
[Rust Edition Guide on unsafe attributes](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html)
explains why symbol uniqueness is an explicit plugin-image obligation.

External consumer tests reject an incorrectly typed factory and each safe
teardown call separately, then compile both macros through ordinary and renamed
facade dependencies. `tests/scene_ownership.rs` executes both consumption paths
with a counted destructor and checks null handling under Miri. These tests do
not establish arbitrary cross-compiler layout compatibility or make unloading
live plugin-backed values safe.

### The host drives reload through a hook it does not name

`flui-app` has no edge to this crate (ADR-0094 §1). `hook.rs` implements
`flui_sdk::view::dev_reload::DevReloadHook` twice, and the application installs
one with `AppConfig::with_dev_reload`. Both sit behind the `host-hook` feature,
the only one that brings in `flui-sdk` (and with it the widget catalog), so a
scene plugin or worker `cdylib` on the default features builds none of it; the
module is also compiled under `cfg(test)`, with `flui-sdk` as a dev-dependency,
so its unit tests run without the feature:

- `WorkerReloadHook` owns the `WorkerReloadDriver`. `attach` starts the
  artifact watcher thread and, with `app-plugin`, registers the
  `request_rebuild` hook, which sets a flag and wakes the host; `poll` runs the
  driver on the owner thread and reports `Patched` for a reload or a pending
  rebuild request; `detach` (and `Drop`) joins the watcher and drops the
  registration. A degraded or failed reload is logged and keeps the last good
  tree, as before.
- `ScenePluginHook` owns the `HotReloadDriver`. `scene_frame` builds the frame's
  scene and lends it to the host's renderer; the scene is dropped before the
  call returns, while the driver still holds the library, which is the ordering
  the `unsafe` `build_scene` requires.

The host applies a `Patched` poll once to every realm, not to whichever realm
polled first; before the hook, a worker's rebuild request reached only the most
recently opened window. Pinned by `hook/tests.rs` here and by
`app/hot_reload/tests.rs` in `flui-app`.
