# ADR-0394: Application code reloads as an image the element tree adopts

- **Status:** Proposed (2026-11-23)
- **Date:** 2026-11-23
- **Related:** [ADR-0393](ADR-0393-a-development-reload-applies-one-of-three-tiers-chosen-from-the-shape-of-the-edit.md)
  (the tiers this mechanism produces and the shape that chooses between them);
  [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) (the hook, and §7's
  exit of the dlopen worker path, which this record would be);
  [ADR-0096](ADR-0096-dev-build-dynamic-linking.md) (the development dylib an image links);
  [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the rejected patcher, option 2 here);
  [ADR-0111](ADR-0111-worker-image-lifetime.md) (an admitted image stays mapped);
  [ADR-0245](ADR-0245-a-view-handed-back-unchanged-is-not-built-again.md) (an element shares its
  view); [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) (the reload tool is an official
  package on `flui-sdk`); [ADR-0097](ADR-0097-no-process-global-state-gate.md) (process-global
  state)
- **Sources:** `DioxusLabs/dioxus@cb27832398e8a3fa30a0fc8d6917c6d8a86fa71a`
  [packages/cli/src/build/patch.rs:1036-1046](https://github.com/DioxusLabs/dioxus/blob/cb27832398e8a3fa30a0fc8d6917c6d8a86fa71a/packages/cli/src/build/patch.rs#L1036-L1046)
  (the Windows x64 stub of option 2); `flutter/flutter@3.50.1`
  `packages/flutter/lib/src/widgets/framework.dart:3772` (`Element.reassemble`)

## Context and problem statement

A reload that keeps `State` exists only in the three-crate project `flui create --hot-reload`
writes: a host that owns the element tree, a worker `cdylib` that holds the `build` functions and
a crate of the types both name ([`docs/hot-reload.md`](../hot-reload.md)). ADR-0311 §7 names a code
patcher as the exit of that path and Subsecond as its candidate; ADR-0094 records why Subsecond is
declined. What bears on a replacement:

| Fact | Evidence |
|---|---|
| The worker links its own copy of the framework: 27 MB, 0.64 s of a 0.92–1.04 s rebuild | run of 2026-11-23, generated hot-reload project, Linux x86_64, 4 cores |
| Every user method is reached through an object the tree owns: an `Element<V, A, B>`, generic over the view type, boxed behind an `ElementBase` sub-trait in `ElementKind` | `crates/flui-view/src/element/unified.rs`, `crates/flui-view/src/element/kind.rs` |
| A view makes its element: `View::create_element(self: Rc<Self>) -> ElementKind` | `crates/flui-view/src/view/view.rs` |
| `#[flui::main]` already writes a second entry where a platform needs one: Android's `android_main`, which hands its `AndroidApp` to the run the function starts | `crates/flui-macros/src/entry.rs`; `hand_android_entry` in `crates/flui-app/src/app/runner/mod.rs` |
| An element built by one build of a crate, handed to a second build of the same crate loaded beside it and coerced there to its trait object again, keeps its allocation and its state and runs the second build's methods; loading and the hand-over took 42 µs | run of 2026-11-23, rustc 1.100.0, Linux x86_64: two `cdylib` builds of one crate against a `dylib` holding the traits |
| In that run a closure that captures another type has another `TypeId`; an `async` block that keeps more across an `.await` keeps its `TypeId` and changes size | the same run |
| Subsecond's Windows x64 stub still clobbers RAX; its linker is part of the `dx` tool and is not published apart from it | Sources; ADR-0094 |

How does a one-crate application load new code into a running process and keep its `State`?

## Decision drivers

- A project from `flui create` reloads with `State` kept, as any Flutter project does; a project
  shape is not the price.
- An author writes widgets as before.
- The memory of an element and its `State` is not copied, serialised or migrated.
- New code does not read a value an older build laid out differently (ADR-0393).
- One copy of the framework's code and statics serves the host and every image, so `TypeId`s agree
  (the reason the worker and host are built in one cargo invocation today).
- The mechanism is stable Rust, cargo and the platform's loader: no assembly, no object-file
  rewriting, no tool the project does not build.
- A release build carries no loader (ADR-0311 §6).

## Considered options

1. Keep the three-crate worker.
2. Subsecond: a patch linked against the running binary's addresses, calls routed through a jump
   table.
3. The application's code is an image linked against the development dylib, and the tree adopts
   its elements into each new image.
4. Keep the process restart and carry `State` across it by serialising it.

## Decision

Proposed option: 3.

§1. **The application's code is an image.**

- The *reloadable crates* are the application's library crate and the workspace members it
  depends on by path. `flui run` builds the library target as a `cdylib`
  (`cargo rustc --lib --crate-type cdylib`), linked dynamically to the development dylib of
  ADR-0096. That file is the *image*. The tool stages it under a content-addressed name, as it
  stages a worker today, and writes beside it a manifest that names the staged file and its shape
  (ADR-0393 §2) in one atomic replacement.
- `#[flui::main]` writes the image's entry in a debug build, as it writes `android_main` on
  Android: an exported function that runs the author's function for a loader.
- The loader lives in `flui-hot-reload`. With the application's `reload` feature the binary's
  `main` is the package's `run_image()`, so the binary holds no application code and is not
  linked again on an edit; without the feature `main` calls the author's function and no loader
  exists. `flui create` writes both, and `flui run` builds with the feature `flui.toml` names.
- The first image's entry starts the event loop with the loader's `DevReloadHook` installed. A
  later image's entry finds the loop running, leaves its root for `DevReloadHook::root` and
  returns. The hand-over between the loader and `Application::run` is the owner thread's
  application slot, the form `hand_android_entry` has; a loader reaches it through an Evolving item
  of `flui_sdk::hooks`.
- So the author's entry function runs again on every reload. Flutter runs `main` again only on a
  hot restart; here new code has no other typed way in.

§2. **The tree adopts its elements into the new image.** In `flui_view`, compiled only with the
reload path:

```rust
pub trait View {
    /// Development reload: take the element an earlier image built for this view type and return
    /// it behind this image's vtable. `Err` hands it back when it is not this type's element.
    fn adopt_element(&self, element: ElementKind) -> Result<ElementKind, ElementKind>;
}
```

- Each implementation sits beside its `create_element` and does one thing: it downcasts the boxed
  element to its own `Element<Self, A, B>` by `TypeId` and coerces it to the sub-trait object
  again. The coercion is compiled in the caller's image, so the same allocation gets that image's
  vtable. An element that owns a render object adopts it the same way through
  `RenderView::RenderObject`.
- In a `Reassemble` pass, the reconciler passes an element through the adopt of the view its
  parent hands it, before it updates the element with that view. The root is adopted through the
  hook's root. An `Err` is a view that cannot update the element: it is unmounted and the view
  mounted, as for any change of type.
- Outside a `Reassemble` pass nothing calls it, and a build without the reload path has no such
  method.

§3. **What guards the adopted memory**, in order:

1. The hook reports `Patched` only when the manifest's shape equals the shape of the image the
   tree was last built from, compared as a typed `Shape`; otherwise it reports `Restart`, which
   adopts nothing.
2. The downcast refuses a different `TypeId`: another generic argument, and a closure whose
   captured types changed.
3. The adopt refuses an element whose size or alignment differs from its type's in the adopting
   image.

What they do not cover: a view or `State` type that holds a future unboxed, when a body edit
reorders the future's interior without changing its size; and a type a statement macro declares
inside a body. The framework's own builders hold futures boxed (`BoxedResultFuture` in
`crates/flui-view/src/element/future_builder.rs`).

§4. **What keeps running the code that made it, and what the tool says.** A value made by code a
`Reassemble` does not run again keeps the code of the image that made it, which stays mapped
(ADR-0111): a closure, listener, timer or future registered in `create_state` or `init_state`, and
a view an application caches and hands back unchanged. Flutter gives such a closure the new code
and, like this, does not run `initState` again.

`flui run` reports it: a `Patched` reload whose changed bodies include a `create_state`, an
`init_state` or the entry function names those functions, says that their edits apply on a restart
and offers it (ADR-0393 §5).

§5. **Statics and dependencies outside the dylib belong to an image.**

- A `static` or `thread_local!` of a reloadable crate starts over in every image. The tool warns
  once, naming each, when a reloadable crate declares one.
- A dependency that is not in the development dylib is linked into every image, so one that keeps
  global state (a runtime, a pool, a registry) starts empty after a reload. The tool lists those
  dependencies once at start. A shared library for them is not built until an application needs
  it.
- The framework's crates and the official packages are in the dylib (ADR-0096 §3): one copy.

§6. **Hosts.** Linux, macOS and Windows desktops and the iOS Simulator load an image. The Android
device path pushes the image as it pushes a scene plugin today, and is a later change. The web has
no loader and keeps `ProcessRestart`.

§7. **This mechanism is the exit ADR-0311 §7 names.** The change that brings it to Linux, macOS and
Windows deletes the worker template and its three-crate project, `WorkerReloadDriver` and the
`WORKER_BUILDS` registry, `HotReloadTier` (the package's copy of `ReloadTier`) and the debt entries
ADR-0311 §7 lists, and rewrites `docs/hot-reload.md`. The scene plugin stays.

### Consequences

- Good, because `flui create` gives a project that reloads with `State` kept.
- Good, because an element's every method moves to the new code at once (`build`, `update`,
  `dispose`, `did_update_view`); a jump table reached 6 of 33 build calls in ADR-0094's spike.
- Good, because the link of an image is small: the framework is not in it.
- Good, because nothing here is outside stable Rust, cargo and `dlopen`/`LoadLibrary`.
- Bad, because the entry function runs again on every reload.
- Bad, because the two cases §3 names are unguarded.
- Bad, because a dependency with global state is duplicated per image until a shared library
  exists.
- Bad, because each reload maps one more image for the life of the process.
- Bad, because it rests on ADR-0096, with its Windows export budget.
- Neutral, because the web gains nothing from it.

### Confirmation

- **Tests the implementing change adds.** In `flui-view`:
  `an_adopted_element_keeps_its_state_and_its_allocation`,
  `an_element_of_another_type_is_handed_back`,
  `an_element_of_another_size_is_handed_back`,
  `a_reassemble_adopts_each_element_once_before_its_update`. In `flui-hot-reload`, against a
  fixture image built twice, the way `reload_lifecycle` builds its fixture today:
  `a_body_edit_keeps_state_and_runs_the_new_build`,
  `a_changed_shape_restarts_and_adopts_nothing`,
  `a_later_image_leaves_its_root_and_starts_no_loop`. In `flui-cli`:
  `a_changed_init_state_is_named_in_the_report`,
  `a_static_in_a_reloadable_crate_is_warned_once`.
- **The generated project.** CI's job that builds a generated project builds it with the `reload`
  feature, loads two images and reads the counter after the second.
- **Runs owed on devices** are rows of `crates/flui-native/platform-parity.toml`: Windows and
  macOS loaders, the iOS Simulator.

## Pros and cons of the options

### 1. Keep the three-crate worker

- Good, because it works on desktop and the iOS Simulator today.
- Bad, because the author pays a project shape, and code that should reload must live in one
  crate and its state's types in another.
- Bad, because the worker carries a second copy of the framework and its statics.

### 2. Subsecond

- Good, because every dependency stays in one copy: the patch calls the running binary's code by
  address.
- Good, because it needs no dylib and patches a wasm target too.
- Bad, because it cannot patch a FLUI application on Windows x64, and the fix is upstream's.
- Bad, because its linker ships only inside `dx`, so `flui run` would drive another tool or write
  that linker again, with assembly stubs for each platform.
- Bad, because a patch reaches only the calls a hook wraps, and patch addresses must be mapped
  back to the first build's.

### 3. An image the tree adopts (proposed)

- Good, because it is built from parts the repository already has: the loader, content-addressed
  staging, images that stay mapped, the reload hook, and an entry the attribute writes.
- Good, because the guard is the type system's downcast plus one comparison the tool owns.
- Bad, because it needs the development dylib first.
- Bad, because an application's own dependencies are per image.

### 4. Carry `State` across a process restart

- Good, because any edit could keep state, a type change included.
- Bad, because every `State` and everything it reaches would have to serialise, which changes how
  widgets are written, and closures, handles and futures cannot.
- Bad, because the restart still costs the window and the start.

## More information

- The record is accepted when the development dylib exists (ADR-0096), the adopt seam and the
  loader pass the tests above on Linux, and a time-boxed Linux run of option 2 has been compared
  with it on four questions: whether `State` survives a body edit, what happens to an application
  that holds an async runtime, how much of `flui run` each needs, and whether Windows is reachable
  without an upstream release.
- Salting the reloadable crate's `-C metadata` with its shape, so that every `TypeId` of a new
  shape differs and the downcast alone refuses, was weighed as §3's first guard. `cargo rustc`
  passes the flag to one crate, not to the workspace members beside it, and a new value starts
  that crate's incremental cache over. The comparison the hook makes needs neither.
- Subsecond's idea of compiling the edited crate without cargo's graph check is independent of
  this choice and is measured apart.
- Revisit §3 when rustc offers a stable hash of a type's layout: the adopt would compare it and
  the two unguarded cases close.
