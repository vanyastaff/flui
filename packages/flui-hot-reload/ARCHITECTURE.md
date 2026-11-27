# Hot-reload plugin boundary

Hot reload is a development-only Rust payload ABI. Host and plugin builds must
agree on layout through the existing ABI-token handshake and keep the plugin
image mapped while plugin-backed values remain alive. This is not a stable C
representation of a scene.

## Mapping decisions

### Rebuild hook generations never return after exhaustion

The hook allocator admits its final nonzero generation once and then refuses
every registration permanently. A refused registration leaves the current hook
installed, so a stale registration guard cannot withdraw it, and the rejected
hook is dropped while the refusal unwinds (ADR-0281). Healthy replacement still
destroys the outgoing hook outside the slot lock.

### Typed scene factories and explicit ownership transfer

The plugin macros export fixed C symbol families once per image. Factories return
the canonical `Scene`, which the package re-exports from `flui_sdk::hooks`. Macro
expansions resolve it through `$crate`, so application authors can invoke either
macro through `flui_hot_reload` or a renamed dependency. No consumer
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
matching consumption path. Null is a no-op.

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

`flui-app` has no edge to this crate (ADR-0311 §6). `hook.rs` implements
`flui_sdk::view::dev_reload::DevReloadHook` twice, and the application installs
one with `AppConfig::dev_reload`. Both sit behind the `host-hook` feature, which
only compiles the hooks: `flui-sdk` is a normal dependency of every build (ADR-0311 §6), so
a scene plugin or worker `cdylib` on the default features links the SDK but builds no
hook. The module is also compiled under `cfg(test)`, so its unit tests run
without the feature:

- `WorkerReloadHook` owns the `WorkerReloadDriver`. `attach` starts the
  artifact watcher thread and, with `app-plugin`, registers the
  `request_rebuild` hook, which sets a flag and wakes the host; `poll` runs the
  driver on the owner thread and reports `Loaded` with the version the
  `WorkerManifest` (`src/manifest.rs`) named for the image it loaded, and
  `reassemble_requested` answers a pending rebuild request; a reload without a
  manifest has no version and reports `Unchanged`; `detach` (and `Drop`) joins the watcher and drops the
  registration. A degraded or failed reload is logged and keeps the last good
  tree, as before.
- `ScenePluginHook` owns the `HotReloadDriver`. `scene_frame` builds the frame's
  scene and lends it to the host's renderer; the scene borrows the driver and
  is dropped before the call returns. The unsafe `scene_frame` caller must
  additionally guarantee the renderer retains no cloned image-dependent
  payload; a borrowed scene alone cannot ensure this.

The host applies each version once to every UI runtime, not to whichever UI runtime
polled first; before the hook, a worker's rebuild request reached only the most
recently opened window. Pinned by `hook/tests.rs` here and by
`app/hot_reload/tests.rs` in `flui-app`.

### The plugin image is a UI runtime of its own for text

`PluginPipeline::mount` takes the `TextContextHandle` its pipeline measures
through (ADR-0092 §10 step 3b), and `app_plugin!` passes
`TextContextHandle::standalone()`: a context over the plugin image's own font
collection, holding the bundled faces. It does not take the host UI runtime's
context. The plugin is a `dlopen`ed image the host reaches only through
`flui_app_build(width, height)`, which has no parameter that could carry the
host's handle, and `abi_token` covers only the `Scene` and `LayerTree` layouts,
so nothing would check that the two images agree on `TextContext`'s layout if
one were passed.

The cost: a face the host app registers does not reach the plugin's text.
Carrying the font bytes across the FFI into the plugin's collection is a
follow-up, unless a code patcher's replacement of the `dlopen` path (ADR-0311 §7) removes the
boundary first.

Pinned by `a_plugin_pipeline_measures_through_the_context_it_is_given`
(`tests/plugin_pipeline_text.rs`, under `app-plugin`).

### The plugin root lays out at each frame's surface size

`PluginPipeline::draw_frame(size)` sets tight root constraints to the size the
host passes to that `flui_app_build` call before it runs the frame, as the host
UI runtime does at its window's size every frame. Constraints set once at mount
would keep the first size after the host's surface is resized. Pinned by
`a_plugin_pipeline_lays_out_at_the_size_of_each_frame`
(`tests/plugin_pipeline_layout.rs`, under `app-plugin`).

A size that is not finite or is negative crosses the ABI as two `f64`s, so
`draw_frame` refuses it with `PluginFrameError::InvalidSize` before anything is
built, and `flui_app_build` hands the host an empty scene with the reason
reported. Pinned by
`a_frame_at_an_invalid_size_is_refused_before_anything_is_built`
(`crates/flui-sdk/src/hooks/plugin_pipeline.rs`).

### Polling an image does not construct a scene

`HotReloadDriver::poll` returns whether an image was loaded or replaced. It
never calls a scene build entry point. Constructing a scene remains the
separate `build_scene` operation, which returns a `PluginScene<'_>` that
borrows the driver: `poll`, which may unload the image, does not compile while
a scene lives. What a borrow cannot hold is a clone of a scene payload
(`Arc<dyn Any>`, `Arc<dyn Fn>`) taken through it, so `build_scene` stays
`unsafe` with that one obligation, the same one `DevReloadHook::scene_frame`
states. The hook polls, builds once, lends the scene to the renderer and
retires it before returning; a reload builds no scene of its own. The driver
and host hook boundaries follow ADR-0108. The consumer test
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

A driver that failed to load an artifact remembers it by that stamp
(`dynlib::LoadRefusal`) and opens the same unchanged file again only after
`LoadRefusal::RETRY`, one second, rather than on every 50 ms read: a broken
build is not reopened twenty times a second on the owner thread, and a failure
outside the bytes (a scanner holding the file, a descriptor limit) still clears
without a rebuild (`a_refused_artifact_is_reopened_once_it_changes_or_after_a_wait`).

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

### Worker descendants keep their admitted image mapped

ADR-0111 retains every successfully admitted worker image until process exit.
A build-call lease cannot protect callbacks, vtables or borrowed storage that
escape into host-owned trees and consumer clones. Worker registration retirement
still prunes matching dispatch entries, but does not invalidate already escaped
values. Rejected images close normally. Each admitted revision consumes image
memory and can keep its Windows staged file locked; restart the host to reclaim
these resources. A scene plugin's image is unloaded on reload, so its scenes
borrow it (`PluginScene`).

`admitted_worker_callbacks_survive_retirement_and_replacement` compiles two
same-toolchain dynamic fixtures and loads them through the public worker API in
a bounded child process. It calls and drops an old image-defined callback after
registration retirement and replacement, then verifies subsequent dispatch and
callback retirement. This test executes the actual loader and plugin code, rather
than inspecting an address or reproducing a retention predicate.

### Worker builds belong to the thread that loads the worker

The registry a worker's init fills is owner-thread state (ADR-0323 §1): a
`thread_local!` `RefCell` holding each fingerprint's newest build and, while an
init runs, the registrations it makes. The host loads a worker and looks its
builds up on the thread that drives its frames; another thread sees an empty
registry. No borrow is held while the worker's init or a `tracing` subscriber
runs, so no call finds the registry borrowed. A `WorkerPlugin` keeps each of
its registrations as a fingerprint and an address integer, which identify the
entry it prunes, so the plugin is `Send` without an `unsafe impl` for the
pointer. `admitted_worker_callbacks_survive_retirement_and_replacement` looks a
build up from a second thread and finds none.

`WorkerReloadHook` is `Send` (`DevReloadHook` requires it), so an application
may make it on a thread other than the loop's. It therefore loads its worker in
`attach`, which the host calls on the loop's thread, not in `new`.
`a_worker_hook_made_on_another_thread_registers_on_the_loop_thread` makes the
hook on a second thread and finds the build on the thread that attached it.

### A worker's rebuild requests reach the host through its init

A worker `cdylib` links its own copy of this crate, statics included, so the
rebuild slot its `request_rebuild` reads is not the one `WorkerReloadHook`
registered the host's hook in. `flui_worker_init` therefore takes the host's
entry (`HostRequestRebuildFn`) beside the registration callback, and
`hot_reload_worker!` binds it into the worker image's slot before the worker's
own init runs. A slot with a hook of its own runs that hook; a slot with only
the host's entry forwards to it. The host's entry runs only its own image's
hook, so an image handed its own entry cannot forward in a loop. The entry is
`extern "C-unwind"`: a panicking hook unwinds into the worker's caller.
Changing a generated symbol's signature changes `SYMBOL_REVISION` in the ABI
token, so an image built before the change is refused at load.

`a_worker_images_rebuild_request_runs_the_hosts_hook`
(`examples/hot_reload_lifecycle_fixture`) loads the cargo-built fixture as a
worker, calls the image's own `request_rebuild` and checks the host's hook ran.

### A panic in the developer's code stays in the plugin image

Every entry a plugin macro generates is `extern "C"`, and a panic cannot
unwind out of one: it aborts the process, the host's included. Each entry that
runs the developer's code (`flui_scene_build`, `flui_app_build` with the root
view's construction, its mount and its frame, and `flui_worker_init`) runs it
through one guard, `entry::contain`. The guard keeps the panic in the image,
writes its text to stderr or logcat (the host's subscriber is not reachable
from the image), and destroys the payload there, where its vtable lives. The
entry answers with its refusal value: a build answers null, which the host
treats as a skipped frame for both families, and an init answers `false`,
which the loader treats as a refused load. A refused worker's registrations
are pruned before its image closes. An `app_plugin!` pipeline that a panic
left unmounted mounts again on the next build.

`a_panicking_scene_function_answers_null_and_the_next_build_works` and
`a_panicking_worker_init_answers_false` (in-process expansions),
`a_panicking_app_root_skips_the_frame_and_the_next_build_mounts` (the
cargo-built fixture), `a_scene_plugin_answering_null_skips_the_frame` and
`a_worker_whose_init_fails_is_refused_with_its_registrations` (`rustc`-built
fixtures) pin it.

### A refused library says why, once per build

`DynLib::open`, `ScenePlugin::load` and `WorkerPlugin::load` answer
`Result<_, LoadError>`. `LoadError::Open` carries the platform loader's own
text (`dlerror`, or the Windows error message), and the other variants name the
missing symbol, both ABI tokens, or the worker's failed init. The drivers load
through `LoadRefusal`, which logs a refusal once per artifact revision, a file
not built yet at `debug` and any other at `warn`, and stays quiet on the
retries of the same revision. Pinned by `a_refused_library_says_why` and
`a_refused_artifact_is_reopened_once_it_changes_or_after_a_wait`.
