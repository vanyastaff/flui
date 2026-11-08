# ADR-0311: Development reload is a hook the application installs

- **Status:** Accepted (2026-11-04)
- **Date:** 2026-11-04
- **Superseded-by:** [ADR-0315](ADR-0315-first-party-packages-remain-train-aligned-facade-features.md)
  for §6's facade prohibition only (the facade keeps its opt-in `hot-reload` feature and re-export;
  the host boundary and every hook decision stand);
  [ADR-0316](ADR-0316-the-dlopen-worker-host-development-cycle-is-supported.md) for §7 only (the
  dynamic-image development cycle is supported, not a temporary path to a code patcher)
- **Related:** [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the rejected Subsecond
  record this one takes the seam from); [ADR-0108](ADR-0108-plugin-scene-render-callback-safety.md)
  (the scene callback's lifetime contract); [ADR-0111](ADR-0111-worker-image-lifetime.md) (worker
  images stay mapped); [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (official packages
  build on `flui-sdk`; its `hooks` module and surface ceiling);
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (the exceptions this record is the exit
  of, and the reach facts); [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the
  runtime names no host or tool); [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md)
  (owner-local state; §5, a thread per runtime later);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) and
  [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (containment of
  user code); [ADR-0136](ADR-0136-owner-local-ui-surfaces.md) §3 (the `Send` positions of the
  public API); [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) §1 (`InputTapFactory`,
  the same factory shape); [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §7
  (the development agent's hooks, which the loop owns the same way); [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the `globals`
  entries); [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  §10 (the lent plugin scene is rendered inline)
- **Sources:** `DioxusLabs/dioxus@cb27832398e8a3fa30a0fc8d6917c6d8a86fa71a`
  (tag `v0.8.0-alpha.3`)
  [packages/cli/src/build/patch.rs:1036-1046](https://github.com/DioxusLabs/dioxus/blob/cb27832398e8a3fa30a0fc8d6917c6d8a86fa71a/packages/cli/src/build/patch.rs#L1036-L1046)
  and [:1107](https://github.com/DioxusLabs/dioxus/blob/cb27832398e8a3fa30a0fc8d6917c6d8a86fa71a/packages/cli/src/build/patch.rs#L1107)

## Context and problem statement

Hot reload reaches a running application through a seam in `flui-view` that names no reload tool.
It was built ahead of the code patcher ADR-0094 proposed, whose spike failed on Windows; ADR-0094
is rejected, so the seam had no accepted record. What `main` holds:

| Fact | Evidence |
|---|---|
| `DevReloadHook: Send + 'static` has `attach`, `detach`, `poll` and an unsafe `scene_frame`; `ReloadEvent` is `Unchanged` or `Patched`; `ReloadWake` is the hook's `Send + Sync` frame request | `crates/flui-view/src/dev_reload.rs` |
| The application installs one instance, `AppConfig::with_dev_reload(impl DevReloadHook)`, and every clone of the configuration shares it | `crates/flui-app/src/app/config.rs` |
| The `Send` bound is there only because the instance travels inside `AppConfig`, which is `Send`; every call is on the owner thread | the trait's docs; `crates/flui-app/src/app/hot_reload.rs` |
| The host keeps the instance in `Arc<Mutex<Slot>>`, lends it out of the slot for each call and catches each call's panic by hand | `crates/flui-app/src/app/hot_reload.rs` (`DevReload`, `lend`, `disable`) |
| Hosts call different methods: desktop and iOS `attach`, `poll` and `detach`; Android only `scene_frame`; the web none. A worker hook does nothing on Android, a scene hook nothing elsewhere | the trait's module table; `runner/desktop.rs`, `runner/ios.rs`, `runner/main_window.rs`, `runner/android.rs` |
| `flui-app` has no edge to `flui-hot-reload` under any feature | `FACTS` in `tools/xtask/src/workspace/reach.rs` |
| `flui-hot-reload` is an official package (`tier-kind = "official"`) still under `crates/`, implementing both hooks behind its `host-hook` feature | `crates/flui-hot-reload/Cargo.toml`, `src/hook.rs` |
| The dlopen path has four internal `edge-exceptions` that exit through §6. Its loader reach and image-local state have standing grants under ADR-0316 and ADR-0097; its process-global routing registries and `flui-view`'s legacy `TEST_REGISTRY` retain per-loop exits through ADR-0316. The facade's separate aggregation is standing under ADR-0315 | the manifests' `[package.metadata.flui]` |

What is the contract of the reload seam, independent of the tool behind it, and which package edges
still leave through its SDK migration?

## Decision drivers

- The host names no reload tool and the tool names no host (ADR-0083, ADR-0088 §2).
- State only the owner thread touches is owner-local: `Send` on a value that never leaves the
  thread, and `Arc<Mutex<_>>` around it, are the "Not" column of AGENTS.md's "Current forms"
  (ADR-0266).
- A hook has one set of calls: a host that accepts it calls all of it, so its author knows what runs
  on every platform it is installed on.
- A hook is user code: a panic, a re-entry or a missing hook costs hot reload, never the frame
  (ADR-0287, ADR-0281).
- Several owner threads may come later (ADR-0266 §5), and one `Send` instance cannot be split
  between them.
- An exception names the change that removes it (ADR-0081 §2, ADR-0097).
- CI executes Linux and wasm32 only, and what a code patcher needs is upstream and on devices.

## Considered options

1. Keep the built shape: one `Send` instance in `AppConfig`, with `scene_frame` on the same trait.
2. A factory in `AppConfig`, a `'static` hook built on each loop's owner thread, and the scene frame
   in a hook of its own.
3. Delete the dlopen path, its exceptions and the seam, and run without hot reload until a code
   patcher arrives.
4. Leave the seam without a record until a code patcher (ADR-0094) is ready, and settle it then.

## Decision

Chosen option: 2, because the seam is independent of the tool and is already held by tests, and
the factory turns "called only on the owner thread" from a promise in the docs into a fact the
compiler checks.

§1. **The reload hook is `'static`, not `Send`.** In `flui_view::dev_reload`, which a package reaches
as `flui_sdk::view::dev_reload`:

```rust
pub trait DevReloadHook: 'static {
    /// Once per event loop, on its owner thread, before the first window.
    fn attach(&mut self, wake: ReloadWake);
    /// The loop is ending. Every `attach` is paired with one `detach`.
    fn detach(&mut self) {}
    /// At each UI runtime's frame boundary: what changed since the last poll.
    fn poll(&mut self) -> ReloadEvent;
}

/// Exhaustive: a host maps every event to the reload it performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadEvent {
    Unchanged,
    /// Every UI runtime of the loop reassembles once; every `State` is kept.
    Patched,
}

/// The one value that crosses threads: a hook's watcher asks for a frame through it.
#[derive(Clone)]
pub struct ReloadWake(Arc<dyn Fn() + Send + Sync>);
```

Without `Send` the compiler keeps the instance on the thread that built it, and a hook keeps its
state in `Rc`, `Cell` and `RefCell` instead of locks and atomics. `ReloadWake` stays `Send + Sync`
because a watcher thread calls it; calling it after its loop has ended does nothing. The trait has
no per-call seam (`call(entry, data)`) and `ReloadEvent` has no `RestartRequired`: if a future code
patcher needs them, its own ADR introduces them without automatically replacing the supported
dynamic-image cycle (ADR-0316).

§2. **The application installs a factory; each event loop builds its own instance on its owner
thread.**

```rust
impl AppConfig {
    pub fn with_dev_reload<H: DevReloadHook>(
        self,
        make: impl Fn() -> H + Send + Sync + 'static,
    ) -> Self;
}
```

- `AppConfig` holds the factory erased (`Arc<dyn Fn() -> Box<dyn DevReloadHook> + Send + Sync>`),
  so the configuration stays `Send` and its clones share the factory, not an instance: the factory
  crosses threads and the hook never does, as with `flui_runtime::InputTapFactory` (ADR-0215 §1).
  A later call replaces the factory.
- An event loop that drives the hook (§4) calls the factory once when it starts, on its owner
  thread, before its first window, and attaches the instance. The loop's owner-local state owns the
  instance and its bookkeeping (`Rc`, `Cell`, `RefCell`; no `static`, no lock). Every UI runtime
  the loop drives is polled through the loop's instance, whatever configuration its window was
  opened with, as the development agent's hooks are handed every window (ADR-0288 §7); opening a
  window never calls the factory. Another loop calls the factory for an instance of its own, so no
  instance is ever attached twice.
- The loop polls the instance at each UI runtime's frame boundary, before the frame's work. A
  `Patched` advances the loop's epoch, and each UI runtime, keyed by its `UiRuntimeId`, reassembles
  (`ReloadTier::Reassemble`) once when its own boundary finds it behind the latest patch. A UI
  runtime is recorded at the current epoch when its root mounts, so one mounted before a patch
  applies it at its first frame and one mounted after does not replay it; an idle window applies a
  patch when it next draws.
- When the loop ends it detaches the instance and destroys it. A `detach` that arrives while a call
  has the instance lent runs when that call returns.
- With no factory installed nothing is built, and the frame boundary costs one `Option` check.

§3. **A scene frame is a hook of its own.**

```rust
pub trait DevSceneHook: 'static {
    /// Draw this frame from a loaded scene plugin instead of the widget tree. `true` means the
    /// host skips the widget pipeline for this frame.
    ///
    /// # Safety
    ///
    /// `render` retains no scene payload, layer clone or annotation whose code or storage belongs
    /// to the plugin image beyond this call, including when it unwinds (ADR-0108).
    unsafe fn scene_frame(
        &mut self,
        width: f64,
        height: f64,
        render: &mut dyn FnMut(&Scene, bool) -> bool,
    ) -> bool;
}

impl AppConfig {
    pub fn with_dev_scene<H: DevSceneHook>(
        self,
        make: impl Fn() -> H + Send + Sync + 'static,
    ) -> Self;
}
```

The method is required: a scene hook that draws nothing has no reason to be installed. The factory
and the instance follow §2: built once per loop on its owner thread, owned by the loop, destroyed
when it ends. The host calls `scene_frame` at the start of every frame on the owner thread, before
the widget pipeline. The callback's second argument is ADR-0108's font namespace reset, and the
lent scene is rendered inline (ADR-0305 §10).

§4. **A host drives every method of each hook it accepts.**

| Host | `DevReloadHook` | `DevSceneHook` |
|---|---|---|
| Windows, macOS, Linux | `attach`, `poll` at each boundary, `detach` | not driven |
| iOS | `attach`, `poll` at each boundary, `detach` | not driven |
| Android | not driven | `scene_frame` every frame |
| Web | not driven | not driven |

A host that does not drive an installed hook never calls its factory and logs once, when its loop
starts, that the hook is not driven there. A desktop scene plugin runs through the package's own
driver in the application's frame loop (`crates/flui-engine/examples/scene_render.rs`), not through the host. Widget
reload on Android and the web is a shortfall in `docs/gaps.md`, not part of this contract.

§5. **A hook's failure costs hot reload, never the frame.** The factory and every hook method are
user code:

- **Lend.** The loop takes the instance out of its slot and commits the slot's bookkeeping before
  the call, so no borrow of the slot is held while the hook runs (ADR-0287 §1). A call that
  re-enters the host, such as a `wake` whose redraw pumps a frame synchronously, finds the slot
  empty: the nested call's outcome is a typed "absent", and it is skipped (ADR-0266), never a
  deadlock or a `BUG:` panic.
- **Panic.** Each call runs inside `FirstPanic::contain`. A panic retires the hook for the rest of
  the loop: its payload goes to `discard_payload`, because nobody resumes a development tool's
  failure into the frame; the `tracing::error!` that reports it runs after the instance is out of
  the slot, contained, since a subscriber is user code; the instance is destroyed with
  `drop_contained`, since its `Drop` may panic too (ADR-0281 §2, §3). The frame that caught the panic
  continues and the UI runtime is untouched.
- **Each point.** The same holds for the factory (the loop runs without that hook), `attach` (the
  hook is retired without a `detach`), `poll`, `detach`, `scene_frame` and the destruction when the
  loop ends. A later loop calls the factory again. A `scene_frame` whose render panics or returns
  `false` keeps the font reset pending (ADR-0108).
- A destructor that panics while the thread is already unwinding aborts, as in std (ADR-0281 §1);
  nothing guards it.

§6. **The reload tool is an official package on `flui-sdk` alone.**

- `flui-hot-reload` lives under `packages/`. Its normal dependencies are `flui-sdk` and the contract
  crates (ADR-0088 §2), with no `edge-exceptions`, and it implements §1 and §3 through
  `flui_sdk::view::dev_reload`.
- The items of the plugin ABI that its `app-plugin` half names (`WidgetsBinding`, `PipelineCell`
  and `PipelineOwner`, `Scene`, `LayerTree`, and the smallest further set the worker needs) are
  Evolving items of `flui_sdk::hooks`, which ADR-0088 §4 keeps outside its surface ceiling, each
  pinned in `crates/flui-sdk/tests/surface.rs`. `flui_view::__runtime` stays out of the SDK, as
  `crates/flui-view/ARCHITECTURE.md` requires. These hooks serve the supported dynamic-image cycle;
  their four internal-crate edges leave through this section's SDK and package move, not through a
  code patcher.
- No core crate other than the facade names the package in any form: `flui-app` and `flui-runtime`
  have no edge to it. The facade keeps its optional `hot-reload` feature and `flui::hot_reload`
  re-export as standing aggregation under ADR-0315 §1–§2. An application enables that feature, and
  `flui create --hot-reload` enables it in the generated application.
- The package reaches the host only through the hooks of §1 and §3; facade aggregation does not
  give `flui-app` a dependency on the package.

§7. **The dynamic-image development cycle is supported under ADR-0316.**

- The worker `cdylib` with its host and types crates implements state-preserving development reload
  on desktop and iOS, and the `--scene` plugin implements the Android scene cycle. These are
  supported development-only workflows, not temporary scaffolding for a code patcher. The
  three-crate worker-host shape is the deliberate cost of image isolation. ADR-0108 and ADR-0111
  hold the lifetime contracts.
- The loader's `windows` and `android_log-sys` reach entries are standing grants of ADR-0316.
  Image-local state is a standing ADR-0097 grant of the `image`, `counter` or `immutable` class as
  ADR-0316 enumerates. `dispatch::REQUEST_REBUILD`, `worker::REGISTRATION_SESSION`,
  `worker::WORKER_BUILDS` and `flui-view`'s `TEST_REGISTRY` retain `exit = "ADR-0316"` until their
  per-loop ownership or production scoped activation migrations land. The four internal-crate
  `edge-exceptions` exit with §6. The facade's separate aggregation edge is standing under ADR-0315.
- Any future code patcher requires its own ADR. It does not automatically delete these workflows or
  revoke the standing grants.

### Consequences

- Good, because "called only on the owner thread" is a type fact: an instance cannot leave the
  thread that built it, and a hook keeps owner-local state without locks.
- Good, because each hook has one set of calls on every host that accepts it, so a worker hook no
  longer sits silently on Android and a scene hook no longer sits silently on the desktop.
- Good, because the supported cycle's intrinsic loader and image costs have standing grants, while
  only its per-loop registries and SDK package edges retain explicit exits; the facade's separate
  aggregation permission needs no exit.
- Good, because a second owner thread gets an instance of its own from the same configuration.
- Bad, because `with_dev_reload` breaks its callers: they pass a closure that builds the hook, and
  the Android `--scene` host moves to `with_dev_scene`.
- Bad, because `cargo xtask send-positions` gains the two factories' `Send` and `Sync` positions
  while losing the hook's `Send` supertrait; they stay because the factory really crosses threads
  inside `AppConfig` (ADR-0136 §3).
- Bad, because `flui create --hot-reload` deliberately retains the three-crate isolation cost, and
  Android and the web have no state-preserving widget reload.
- Neutral, because the reload tiers stay `flui_runtime::reload::ReloadTier`, and only `Reassemble`
  has a producer.

### Confirmation

- **The compiler.** Both hooks are `'static` alone. The implementing change adds a doctest on
  `AppConfig::with_dev_reload` whose hook holds an `Rc` (it compiles only without `Send`), and a
  `compile_fail` doctest that calls `scene_frame` on a `dyn DevReloadHook`; the existing
  `compile_fail,E0133` doctest of a safe `scene_frame` call moves to `DevSceneHook`.
- **`cargo xtask send-positions`.** The `flui_view::dev_reload::DevReloadHook` supertrait entry of
  `tools/xtask/allowlists/send-positions.toml` is deleted, so a `Send` bound returning on either hook
  fails the gate; the factories' positions are listed.
- **`cargo xtask reach`.** The facts `flui-hot-reload must be absent from flui-app's default graph`
  and `flui-hot-reload must be absent from flui-app's graph with every feature` hold §6 for the
  host. The facade's presence fact remains: its standing `hot-reload` feature and re-export do not
  become absence facts. The two loader entries hold `grant = "ADR-0316"`.
- **`cargo xtask workspace`.** It refuses `edge-exceptions` under `packages/`, so the package's move
  there proves "on `flui-sdk` alone". Until then its four internal edges retain
  `exit = "ADR-0311"`. `cargo xtask globals` keeps the standing image-local grants on
  `grant = "ADR-0097"` and the four named registry entries on `exit = "ADR-0316"`.
- **Tests that exist.** The host's `dev_reload_hook_matrix` (`crates/flui-app/src/app/hot_reload/tests.rs`):
  `a_patch_polled_through_the_hook_reassembles_the_ui_runtime`,
  `without_a_hook_the_frame_is_unchanged`, `one_patch_reaches_every_ui_runtime_exactly_once`,
  `a_ui_runtime_first_polled_after_a_patch_does_not_replay_it`,
  `a_ui_runtime_mounted_before_a_patch_applies_it_at_its_first_boundary`,
  `a_panicking_poll_disables_the_hook_and_the_frame_continues`,
  `a_detach_that_arrives_during_a_hook_call_runs_when_the_call_returns`,
  `a_panicking_attach_leaves_the_loop_without_reload` and
  `each_reload_event_maps_to_the_ui_runtime_tier`; the runner's
  `main_window_reload_hook_stays_attached_across_failed_reopens_and_detaches_with_loop`. The
  package's (`crates/flui-hot-reload/src/hook/tests.rs`): `event_for_maps_each_driver_outcome`,
  `a_rebuild_request_polls_as_a_patch_and_wakes_once_until_detached`,
  `an_artifact_change_wakes_the_host`, `dropping_an_attached_hook_joins_its_watcher`,
  `scene_frame_without_a_loaded_plugin_draws_nothing` and `scene_frame_reset_recovery_matrix`.
  `flui-view`'s `a_wake_clone_calls_the_same_host_request`.
- **Tests the implementing change adds.** The factory is called once per loop, on that loop's
  thread, and never by a host that does not drive its hook; a panicking factory leaves that loop
  without reload and the next loop calls it again; two loops started from clones of one
  configuration each attach an instance of their own, which replaces the refusal half of
  `attach_and_detach_pair_once_per_loop_and_a_second_attach_is_refused`; the scene hook's lend and
  containment run on the Linux host, not only under `cfg(target_os = "android")`.
  `scene_frame_default_never_calls_render` goes with the default it pins.

## Pros and cons of the options

### 1. Keep the built shape

- Good, because nothing changes and the tests already pass.
- Bad, because `Send` stays on a value that never leaves the owner thread, and the host keeps it
  behind `Arc<Mutex<_>>`, both in the "Not" column.
- Bad, because one trait holds two roles that no host calls together: a worker hook does nothing on
  Android, a scene hook nothing on the desktop, and the trait's docs carry the table that says so.
- Bad, because one instance cannot serve two owner threads.

### 2. A factory, an owner-thread hook and a scene hook of its own (chosen)

- Good, because the compiler holds the owner-thread rule, and owner-local state replaces the lock.
- Good, because each hook's calls are the same on every host that drives it.
- Good, because the shape is the one `InputTapFactory` already has.
- Bad, because it breaks `with_dev_reload`'s callers and adds the factories' `Send` positions.

### 3. Delete the dlopen path and run without hot reload

- Good, because it is the cleanest: the fourteen dlopen-path exceptions, the `ManuallyDrop`
  registry stack and the plugin ABI leave at once; the facade's aggregation permission remains.
- Bad, because it takes away a feature that works (a logic edit reloads with `State` kept, on
  desktop and iOS) for as long as the code patcher's condition lies upstream.
- Neutral, because it is reversible, as option 2 is.

### 4. Wait for a code patcher

- Good, because the seam would be settled once, with the per-call routing a patcher needs.
- Bad, because fourteen dlopen-path exceptions keep naming a record that is not a rule, with no
  date or condition under the project's control, and the seam the tests hold stays undecided.

## More information

- **Revisit** this record when ADR-0266 §5 brings a thread per runtime: the factory may then be
  called per runtime instead of per loop.
- ADR-0094 retains the rejected Subsecond spike and its evidence. It supplies no return condition
  for this record or for ADR-0316's supported development cycle.
- The factory shape: `flui_runtime::InputTapFactory` (`crates/flui-runtime/src/input_tap.rs`).
