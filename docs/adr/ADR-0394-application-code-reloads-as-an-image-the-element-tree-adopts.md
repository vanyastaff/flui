# ADR-0394: Application code reloads as an image the element tree adopts

- **Status:** Proposed (2026-11-23)
- **Date:** 2026-11-23
- **Related:** [ADR-0393](ADR-0393-a-development-reload-applies-one-of-three-tiers-chosen-from-the-shape-of-the-edit.md)
  (the tiers, the version a hook reports and what a restart does);
  [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) (the hook, and §7's
  exit of the dlopen worker path, which this record would be);
  [ADR-0096](ADR-0096-dev-build-dynamic-linking.md) (the development dylib an image links);
  [ADR-0326](ADR-0326-one-entry-starts-an-application-on-every-platform.md) (the entry, which
  gains a second export in a development build);
  [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the rejected patcher, option 2 here);
  [ADR-0111](ADR-0111-worker-image-lifetime.md) (an admitted image stays mapped);
  [ADR-0245](ADR-0245-a-view-handed-back-unchanged-is-not-built-again.md) (an element shares its
  view); [ADR-0097](ADR-0097-no-process-global-state-gate.md) (process-global state)
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
| Every user method is reached through an object the tree owns: an `Element<V, A, B>`, generic over the view type, boxed behind an `ElementBase` sub-trait in `ElementKind`. A render view's element holds a `RenderId`, and the boxed render object lives in the render tree | `crates/flui-view/src/element/unified.rs`, `crates/flui-view/src/element/kind.rs` |
| A view makes its element in `View::create_element(self: Rc<Self>) -> ElementKind`, which the `StatelessView` and `StatefulView` derives and the `impl_*_view!` macros write | `crates/flui-view/src/view/view.rs`, `crates/flui-macros/src/lib.rs` |
| `#[flui::main]` already writes a second entry where a platform needs one: Android's `android_main`, which hands its `AndroidApp` to the run the function starts | `crates/flui-macros/src/entry.rs`; `hand_android_entry` in `crates/flui-app/src/app/runner/mod.rs` |
| A route keeps the closure that builds its page, and an element handed the view it already holds is not updated | `page_builder` in `crates/flui-widgets/src/navigator/page_route.rs`; ADR-0245 |
| An element built by one build of a crate, handed to a second build of the same crate loaded beside it and coerced there to its trait object again, keeps its allocation and its state and runs the second build's methods; loading and the hand-over took 42 µs | run of 2026-11-23, rustc 1.100.0, Linux x86_64: two `cdylib` builds of one crate against a `dylib` holding the traits |
| In that run a closure that captures another type has another `TypeId`; an `async` block that keeps more across an `.await` keeps its `TypeId` and changes size | the same run |
| An exported function can be named from its type's path, `concat!("…", module_path!(), "::", stringify!(T))`, which equals a prefix and `std::any::type_name::<T>()` for a type declared in a module | run of 2026-11-23, rustc 1.100.0 |
| Subsecond's Windows x64 stub still clobbers RAX; its linker is part of the `dx` tool and is not published apart from it | Sources; ADR-0094 |

How does a one-crate application load new code into a running process and keep its `State`?

## Decision drivers

- A project from `flui create` reloads with `State` kept, as any Flutter project does; a project
  shape, a feature or a line of `main` is not the price.
- An author writes widgets as before, and the entry runs when Flutter's `main` runs.
- The memory of an element and its `State` is not copied, serialised or migrated.
- New code does not read a value an older build laid out differently (ADR-0393).
- Code that an edit replaced and that still runs is named by the process that runs it, not
  guessed.
- One copy of the framework and of every dependency serves every image (ADR-0096).
- The mechanism is stable Rust, cargo and the platform's loader: no assembly, no object-file
  rewriting, no tool the project does not build.
- A built application carries no loader (ADR-0311 §6).

## Considered options

1. Keep the three-crate worker.
2. Subsecond: a patch linked against the running binary's addresses, calls routed through a jump
   table.
3. The application's code is an image linked against the development dylib, and the tree adopts
   its elements into each new image.
4. Keep the process restart and carry `State` across it by serialising it.

## Decision

Proposed option: 3.

§1. **The application's code is an image, started by a launcher the tool writes.**

- The *reloadable crates* are the application's library crate and the workspace members it
  depends on by path. `flui run` builds the library target as a `cdylib`
  (`cargo rustc --lib --crate-type cdylib`) against the development dylib, in the tool's own
  build (ADR-0096 §3). That file is the *image*. The tool stages it under a
  content-addressed name, as it stages a worker today, and writes beside it a manifest that names
  the staged file and the `CodeVersion` the tool gave the build (ADR-0393 §2), in one atomic
  replacement.
- The process is the launcher of ADR-0096 §3. Its `main` is the reload package's: it loads the
  first image, runs its entry, and starts the event loop with the package's `DevReloadHook`
  installed. The application's manifest, its `main.rs` and its features do not change, and the
  application does not depend on the reload package.
- Under `cfg(flui_dev_reload)`, `#[flui::main]` writes the image's entry beside the author's function,
  as it writes `android_main` on Android: an exported function that runs the author's function
  for the loader. `Application::run` in an image hands its application and root to the loop that
  ran the entry and returns, the form `hand_android_entry` has; the loader reaches that slot
  through an Evolving item of `flui_sdk::hooks`.
- The entry runs when the application is composed: at the start and at each restart (ADR-0393
  §4), where `DevReloadHook::run_entry` runs the entry of the image now loaded. It does not run
  on a reassemble.
- The hook reports `Loaded(version)` once the image named by a new manifest is mapped. An image
  the loader cannot map is reported to the tool, which applies a process restart, and the hook
  reports `Unchanged`.

§2. **The tree adopts its elements into the image now loaded.** One primitive in
`flui_view::reload`, compiled only under `cfg(flui_dev_reload)`, which the tool passes to every
crate of its own build and to no other:

```rust
/// Take an element an earlier image built and return it behind this image's vtable.
/// Monomorphized in the image that calls it, which is what moves the element.
pub fn adopt<E: ElementBase>(element: ElementKind) -> Adoption;

#[non_exhaustive]
pub enum Adoption {
    /// The same allocation, behind the calling image's vtable.
    Adopted(ElementKind),
    /// Not an `E`: handed back untouched.
    NotThis(ElementKind),
    /// An `E` by `TypeId` whose size or alignment differs: handed back untouched.
    Refused(ElementKind),
}

pub trait View {
    /// Adopt the element an earlier image built for this view type. The default hands it back
    /// as `NotThis`.
    fn adopt_element(&self, element: ElementKind) -> Adoption {
        Adoption::NotThis(element)
    }
}
```

New code reaches an element in two ways, and both end in `adopt`:

- **By the element's type.** For every view type they are on that has no generic parameter and is
  declared in a module, the derives and the `impl_*_view!` macros write, under that `cfg`,
  an exported adopter whose name is built from the type's path. The loader resolves it in the
  image now loaded from the element's view type name. The name only finds the function: the
  adopter's own `TypeId` check decides.
- **By the view its parent hands it.** The same derives and macros write `adopt_element` beside
  `create_element`. It serves a generic view type and a type declared inside a body, which have
  no exported adopter.

An element is *behind* when its code is not in the image now loaded: each element answers an
address inside the code that implements it, and the loader knows each image's address range.
The loader lends the build owner both answers through the hook, `DevReloadHook::image_code`, which
this record adds with a default of `None`:

```rust
/// What a loader of images lends the build owner for a reassemble and the frames after it.
pub trait ImageCode {
    /// Whether `code`, an address an element answered, is in the image now loaded.
    fn is_current(&self, code: CodeAddress) -> bool;
    /// The adopter the image now loaded exports for the view type named `view`.
    fn adopter(&self, view: &'static str) -> Option<Adopter>;
}
```

- In a reassemble the walk that marks every element dirty adopts each element that is behind, by
  its type, before any element rebuilds. An element still behind is adopted by its parent's view
  when the parent hands it one of the current image.
- After the pass the same two ways apply to any element found behind when it is updated or
  rebuilt: one a retained closure of an earlier image built, a kept-alive or lazily built child,
  a presentation that builds later.
- The adopt of a render view's element also adopts its render object: the box is taken out of
  its node of the render tree, coerced in the adopting image and put back, and so is a parent
  data box of an application's type.
- `Refused` raises the reassemble to a restart with the view type as the reason (ADR-0393 §3).
  The refused element is not updated, so until the restart each element runs code that matches
  its memory.
- The elements still behind when the frame ends are the outcome's `behind` list, by view type.

A hand-written `View` that takes neither a derive nor a macro keeps the default: its elements
stay behind and the report names the type.

§3. **What guards the adopted memory**, in order:

1. The loop reassembles only to a version whose `layout` equals the one its trees were composed
   from, and the tool gives a build a new layout whenever a reloadable crate's shape changed
   (ADR-0393 §2 and §3). The loader compares nothing.
2. `adopt` refuses a different `TypeId`: another generic argument, and a closure whose captured
   types changed.
3. `adopt` refuses an element whose size or alignment, read through the vtable it arrives with,
   differs from the type's in the adopting image.

What they do not cover: a view or `State` type that holds a future unboxed, when a body edit
reorders the future's interior without changing its size; and a type a statement macro declares
inside a body, when an edit changes its fields without changing its size. The framework's own
builders hold futures boxed (`BoxedResultFuture` in
`crates/flui-view/src/element/future_builder.rs`).

§4. **What keeps running the code that made it, and how the report names it.**

- An element that could not be adopted is in `behind`, and the tool prints its view type and
  offers a restart. This is what the process observed, so it covers a hand-written `View`, a
  generic view a retained closure builds and a cached view of a generic type.
- A closure, listener, timer or future registered in `create_state` or `init_state` keeps the code
  of the image that made it, which stays mapped (ADR-0111), and so does the body of a closure a
  `State` retains, such as a route's page builder. Flutter gives such a closure the new code and,
  like this, does not run `initState` again. The process cannot see them, so the tool names what
  it can from the edit: a reassemble whose changed bodies include a `create_state`, an
  `init_state`, the entry function or a closure inside one of them says that those edits apply on
  a restart and offers it.
- The tool counts the images a process has loaded and applies a process restart with the edit
  after the two hundredth, saying why.

§5. **Statics of a reloadable crate belong to an image.** A `static` or `thread_local!` of a
reloadable crate starts over in every image. The tool warns once, naming each, when a reloadable
crate declares one. Every other crate is in the development dylib, in one copy (ADR-0096 §3).

§6. **Hosts.** The Linux, macOS and Windows desktops load an image. The iOS Simulator loads one
once the tool builds the dylib and the launcher for it and copies images into the application's
container, a later change. Android keeps the scene plugin and a process restart for widget code,
and the web has no loader.

§7. **This mechanism is the exit ADR-0311 §7 names, in two steps.** The change that brings it to
the three desktops deletes the worker template and its three-crate project, the
`hot_reload_counter` and `hot_reload_lifecycle_fixture` examples and `HotReloadTier`, and rewrites
`docs/hot-reload.md`. `WorkerReloadDriver`, the `WORKER_BUILDS` registry and the debt entries
ADR-0311 §7 lists serve the iOS Simulator until it loads images: that change marks them
`#[deprecated]` with an entry in `docs/gaps.md`, and the change that brings images to the
Simulator deletes them. The scene plugin stays.

### Consequences

- Good, because `flui create` gives a project that reloads with `State` kept, with nothing in it
  that exists for reload.
- Good, because an adopted element's every method moves to the new code at once (`build`,
  `update`, `dispose`, `did_update_view`); a jump table reached 6 of 33 build calls in ADR-0094's
  spike.
- Good, because the entry runs when Flutter's `main` runs.
- Good, because the link of an image is small: nothing but the reloadable crates is in it.
- Good, because nothing here is outside stable Rust, cargo and `dlopen`/`LoadLibrary`.
- Bad, because code a kept value captured does not move: a closure made in `create_state`, in
  `init_state` or in a handler keeps its body until a restart, where Flutter replaces it. A jump
  table has the same limit for a call no hook wraps.
- Bad, because an element of a generic or hand-written view type that no current parent hands a
  view stays behind until a restart.
- Bad, because the two cases §3 names are unguarded.
- Bad, because statements after `run` in the entry execute when the application is handed over,
  not when the loop ends.
- Bad, because each reload maps one more image, up to §4's bound.
- Bad, because it rests on ADR-0096, with its Windows export budget.
- Neutral, because the web gains nothing from it.

### Confirmation

Each decision and what holds it. "Two images" is a fixture crate built twice, the way
`reload_lifecycle` builds its fixture today, in `flui-hot-reload`'s suite.

| Decision | Held by |
|---|---|
| §1 the entry runs at the start and at a restart only | two images: `a_reassemble_does_not_run_the_entry`, `a_restart_runs_the_entry_of_the_image_now_loaded` |
| §1 an image that cannot be mapped | `flui-hot-reload`: `an_unmappable_image_is_reported_and_nothing_reloads` |
| §2 `adopt` | `flui-view`: `an_adopted_element_keeps_its_state_and_its_allocation`, `an_element_of_another_type_is_handed_back` |
| §2 every way to implement `View` writes its adopt | `flui-view` table test `each_way_to_implement_view_adopts_its_own_element`, a row per derive and per `impl_*_view!` macro; the default is a row that expects `NotThis` |
| §2 by type and by view, in and after the pass | two images: `a_body_edit_keeps_state_and_runs_the_new_build`, `an_element_built_by_a_retained_closure_is_adopted_by_its_type`, `a_view_handed_back_unchanged_is_adopted_by_its_type`, `an_element_updated_after_the_pass_is_adopted_then`, `a_generic_view_is_adopted_by_its_parents_view` |
| §2 a render object | two images: `a_body_edit_to_a_render_object_runs_the_new_paint` |
| §2 `behind` | two images: `an_element_left_behind_is_named_in_the_outcome` |
| §3 guard 3, and the raise | two images, with a view that holds an unboxed future whose size the edit changes: `an_element_whose_size_changed_raises_a_restart` |
| §4 the tool's part of the report | `flui-cli`: `a_changed_init_state_is_named_in_the_report`, `the_two_hundredth_image_restarts_the_process` |
| §5 | `flui-cli`: `a_static_in_a_reloadable_crate_is_warned_once`; two images: `a_static_of_a_dependency_is_shared_by_both_images` |
| Nothing of it outside the tool's build | the compiler: without `cfg(flui_dev_reload)` `flui-view` has no `reload` module and `View` no `adopt_element`, and the workspace's lints name the `cfg` |
| The generated project | a `flui-cli` scenario generates a project, loads two images and reads the counter after the second |

Runs owed on devices are rows of `crates/flui-native/platform-parity.toml`: the Windows and macOS
loaders, export names that contain `::` on Windows, the iOS Simulator.

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
- Good, because a function reached through its table is replaced wherever a kept value points at
  the table.
- Bad, because it cannot patch a FLUI application on Windows x64, and the fix is upstream's.
- Bad, because its linker ships only inside `dx`, so `flui run` would drive another tool or write
  that linker again, with assembly stubs for each platform.
- Bad, because a patch reaches only the calls a hook wraps, and patch addresses must be mapped
  back to the first build's.

### 3. An image the tree adopts (proposed)

- Good, because it is built from parts the repository already has: the loader, content-addressed
  staging, images that stay mapped, the reload hook, and an entry the attribute writes.
- Good, because the guard is the type system's downcast plus a version the tool owns.
- Good, because every dependency is in one copy, in the dylib.
- Bad, because it needs the development dylib first.
- Bad, because code a kept closure captured stays old, and only an element can be found behind.

### 4. Carry `State` across a process restart

- Good, because any edit could keep state, a type change included.
- Bad, because every `State` and everything it reaches would have to serialise, which changes how
  widgets are written, and closures, handles and futures cannot.
- Bad, because the restart still costs the window and the start.

## More information

- The record is accepted when the dylib of ADR-0096 exists, the tests above pass on Linux, and a
  time-boxed Linux run of option 2 has been compared with it on four questions, each with the
  answer that blocks acceptance: `State` survives a body edit (it must); a `State` that holds an
  async runtime's handle still spawns after a body edit (it must); the part of `flui run` each
  needs (the image's must not need a tool the project does not build); Windows is reachable
  without an upstream release (it must be).
- Weighed inside option 3, where new code gets its way in. Running the entry again on every
  reload gives each image a typed root and was what the first run did; it repeats whatever the
  entry does before `run` (a second log subscriber, a second runtime, a second bind) on every
  save, and Flutter runs `main` only on a hot restart. Adopters found by type need no root and
  also reach an element a retained closure built, at the price of an exported name per view type.
- Weighed as §3's first guard: salting the reloadable crate's `-C metadata` with its shape, so
  that every `TypeId` of a new shape differs and the downcast alone refuses. `cargo rustc` passes
  the flag to one crate, not to the workspace members beside it, and a new value starts that
  crate's incremental cache over. The version the tool stamps needs neither.
- Subsecond's idea of compiling the edited crate without cargo's graph check is independent of
  this choice and is measured apart.
- Revisit §3 when rustc offers a stable hash of a type's layout: the adopt would compare it and
  the two unguarded cases close.
