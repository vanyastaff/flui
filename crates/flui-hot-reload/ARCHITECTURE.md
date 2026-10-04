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

This follows Rust's ownership rules:
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
  scene and lends it to the host's renderer. The original scene is dropped
  before the call returns while the driver holds the image. The unsafe
  `scene_frame` caller must additionally guarantee the renderer retains no
  cloned image-dependent payload; a borrowed scene alone cannot ensure this.

The host applies a `Patched` poll once to every realm, not to whichever realm
polled first; before the hook, a worker's rebuild request reached only the most
recently opened window. Pinned by `hook/tests.rs` here and by
`app/hot_reload/tests.rs` in `flui-app`.

### The plugin image is a realm of its own for text

`PluginPipeline::mount` takes the `TextContextHandle` its pipeline measures
through (ADR-0092 §10 step 3b), and `app_plugin!` passes
`TextContextHandle::standalone()`: a context over the plugin image's own font
collection, holding the bundled faces. It does not take the host realm's
context. The plugin is a `dlopen`ed image the host reaches only through
`flui_app_build(width, height)`, which has no parameter that could carry the
host's handle, and `abi_token` covers only the `Scene` and `LayerTree` layouts,
so nothing would check that the two images agree on `TextContext`'s layout if
one were passed.

The cost: a face the host app registers does not reach the plugin's text.
Carrying the font bytes across the FFI into the plugin's collection is a
follow-up, unless ADR-0094's replacement of the `dlopen` path removes the
boundary first.

Pinned by `a_plugin_pipeline_measures_through_the_context_it_is_given`
(`tests/plugin_pipeline_text.rs`, under `app-plugin`).

### The plugin root lays out at each frame's surface size

`PluginPipeline::draw_frame(width, height)` sets tight root constraints to the
size the host passes to that `flui_app_build` call before it runs the frame, as
the host realm does at its window's size every frame. The pipeline laid nothing
out without root constraints, and constraints set once at mount would keep the
first size after the host's surface is resized. Pinned by
`a_plugin_pipeline_lays_out_at_the_size_of_each_frame`
(`tests/plugin_pipeline_layout.rs`, under `app-plugin`).

### Polling an image does not construct a scene

`HotReloadDriver::poll` returns whether an image was loaded or replaced. It
never calls a scene build entry point. Constructing a scene remains the
separate unsafe `build_scene` operation: its caller must drop every scene and
retained clone before the image can be unloaded. A safe polling method that
returned an unrestricted `Scene` would let that lifetime obligation escape
without an unsafe call. The hook polls, builds once, lends the scene to the
renderer and retires it before returning; reload no longer builds and discards
an extra scene. The driver and host hook boundaries follow ADR-0108. The consumer test
`polling_an_unavailable_plugin_reports_no_change` pins the polling result type
and retries an unavailable image without constructing a scene.

Unix loading preserves native path bytes rather than requiring UTF-8. Symbol
names remain UTF-8 C strings, a separate API input. The Linux GNU consumer test `a_native_byte_library_path_can_be_loaded` opens
an already-loaded libc image through a non-UTF-8 symlink and resolves its
`malloc` symbol. It needs no compiled plugin fixture. Unix native-byte loading has not
been executed on the current Windows development host.

### Reload detection retains native timestamp precision

Loaded scene and worker plugins, and the idle artifact watcher, compare the
same native `Option<SystemTime>` metadata revision. Whole-second truncation
would miss two replacements of the same artifact path within one second.
Missing metadata is a distinct unavailable revision; recreation changes it
again. Resolved paths still participate in the watcher's identity, so staged
content-addressed filenames remain distinguishable even with equal timestamps.
The public `worker_artifact_stamp` returns the resolved path and optional native
timestamp, and is the method the watcher uses. The former seconds-only
`file_mtime` helper is removed because no production consumer needs its
truncated timestamp.
`same_path_subsecond_revisions_and_recreation_are_detected` uses actual
`FileTimes` updates within one second, then deletion and recreation. The existing
`an_artifact_change_wakes_the_host` additionally pins watcher delivery.
The test invokes the public stamp without loading an image; loader and watcher
share its native timestamp detector. It does not reconstruct the stamp predicate.

### Plugin image font identities reset until rendering succeeds

A plugin image can issue the same font blob IDs as its predecessor for different
font bytes. `ScenePluginHook` requests a fresh renderer font namespace on its
first scene and after every successful image replacement. The request remains
pending until the callback returns true; false or unwind does not acknowledge
it, and an unchanged-image poll cannot erase it. Replacing the entire hook also
starts pending. The callback receives the request alongside its scene borrow.

`scene_frame_reset_recovery_matrix` pins first/replacement success, refusal,
panic and the next render. Its private seam invokes the production image-update
and callback-dispatch methods with a borrowed empty scene because installing a
real replacement DSO is not needed to test acknowledgement chronology. It
asserts callback flags and results, not internal fields or a copied predicate.
