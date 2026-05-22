# Rust UI Ecosystem Lessons for FLUI

**Date:** 2026-05-22
**Author:** External consultant (Rust UI ecosystem veteran)
**Feeds:** Master ROADMAP — parallel input alongside gap-matrix, port-phasing, and architectural-contracts documents

---

## Intro and Methodology

This document is the outside-in complement to the internal audit chain already in `docs/research/`. It answers a single question: **what has the broader Rust UI ecosystem already learned, at cost, that FLUI should not re-learn?**

### What was read

- GPUI source vendored at `.gpui/src/` — `app.rs`, `app/context.rs`, `element.rs`, `platform.rs`, and the flat entity map; read directly from the worktree file system.
- DeepWiki queries against: `linebender/xilem`, `iced-rs/iced`, `makepad/makepad`, `DioxusLabs/dioxus`, `emilk/egui`, `slint-ui/slint`, `lapce/floem`, `linebender/vello`, `zed-industries/zed`.
- Primary blog posts: Raph Levien "Xilem: an architecture for UI in Rust" (2022-05-07); "Advice for the next dozen Rust GUIs" (2022-07-15); "Towards principled reactive UI" (2020-09-25); Colin Rofls "Some reflections on the Druid architecture"; Zed blog "Ownership and data flow in GPUI" (2024-01-25).
- Linebender RFC `rfcs/0001-masonry-backend.md`; `xilem 0.4.0` `ARCHITECTURE.md` on docs.rs; Linebender 2024 backend roadmap post.
- Flutter internals: `inside-flutter.md` from the Flutter website repo (linear reconciliation, InheritedWidget hash table, setState dirty-element list).
- Floem docs and `lapce/floem` README; Dioxus signals architecture notes (`notes/architecture/04-SIGNALS.md`).
- Vello README and GitHub issues for sparse-strip rendering (#670) and `vello_hybrid` PR (#831).
- A 2025 survey of Rust GUI libraries (boringcactus.com, April 2025).

Fact is marked as fact when it comes from source code or primary writing. Inference is labeled.

---

## 1. GPUI (Zed editor)

### Architecture model

GPUI is a **hybrid immediate/retained** framework. There is no retained widget tree in the Flutter/Xilem sense. Instead, every frame the `Render::render()` root rebuilds the full element tree from scratch; the tree is dropped at frame end. Persistence comes from **entities** — typed state objects (models and views) owned by a single flat `App` store. Access is via reference-counted handles `Entity<T>` that are inert identifiers until paired with a `&mut App`. The `Context<T>` type is `&mut App` specialized to an entity, obtained via the **lease pattern**: entity state is temporarily removed from `App` into a stack-local, the callback fires with both `&mut T` and `&mut Context<T>`, then the lease is returned. Effect queuing (`flush_effects`) defers observer notifications to end-of-update, giving run-to-completion semantics with no reentrancy bugs.

Layout uses Taffy (flexbox, CSS-inspired), not a Flutter-style constraint-propagation protocol. There is no three-tree lifecycle at the widget level; `Element::request_layout` / `Element::prepaint` / `Element::paint` are the three passes per element per frame.

### What GPUI got right

- **Ownership clarity.** App-owns-all eliminates the aliasing hell that plagued earlier Rust UI work. The lease pattern is the cleanest published solution to the "need `&mut T` and `&mut Cx` simultaneously" problem in Rust.
- **Effect queue.** `flush_effects` prevents the Atom-style reentrancy bugs (observed in production at GitHub before Atom was retired). Run-to-completion is correct for UI update semantics.
- **Production proof.** Zed is a shipping, performant desktop application; GPUI's model scales to a real codebase.

### Hard problems GPUI hit

- **Not retained-mode at the widget level.** Rebuilding the full element tree every frame means accessibility trees, IME state, and focus rings must be reconstructed or re-identified each frame. GPUI handles this via stable `ElementId` persistence, but it is ongoing work.
- **Single-threaded App.** `App` is `Rc<RefCell<App>>` (confirmed in `app.rs` line 63 — `AppCell` wraps `RefCell<App>`). No `Send`. This is an explicit choice (the `Entity` handles are `Clone` + `'static` but the update cycle is single-threaded). Multi-window Zed runs multiple processes rather than sharing state across threads.
- **Element regeneration cost.** "The entire element tree and any callbacks they have registered with GPUI are dropped before the start of the next frame" (`element.rs` module doc). At Zed's codebase size this is manageable because most elements are cheap value types; but a deeply dynamic UI with large trees would pay more.

### Relevance to FLUI

- **Do not adopt GPUI's element model.** FLUI's three-tree (View → Element → Render) with a persistent Element/Render tree is architecturally superior for Flutter-parity. The persistent element tree is where `State`, element keys, and layout cache live; discarding it each frame requires re-identification work GPUI does through `ElementId` heuristics. FLUI's Slab-based retained trees are the right shape.
- **Adopt the lease pattern** for any situation where a component needs `&mut self` and `&mut Cx` simultaneously. The pattern is directly applicable in `flui-view`'s `BuildContext`.
- **Adopt the effect queue pattern** for event dispatching: queue notifications, flush at end-of-frame. Prevents reentrancy in `BuildOwner::flush_dirty_elements()` equivalents.
- **`App`-owns-all for non-widget entities** (platform resources, asset caches, text system): GPUI's model is clean for these even if FLUI's widget tree owns widgets differently.

---

## 2. Xilem / Masonry / Linebender

### Architecture model

Xilem is the **most architecturally relevant external project** for FLUI. It is a two-layer system:

1. **Reactive layer (Xilem):** A lightweight, transient `View` tree is generated by `app_logic(app_state)` on each cycle. `View` is a Rust value type (not a trait object) that implements `build()`, `rebuild()`, `teardown()`, and `message()`. `rebuild(prev, element, state)` receives the previous view for diffing and mutates the retained element in place. The tree is retained long enough to dispatch events and diff against the next cycle, then dropped.

2. **Retained layer (Masonry):** A persistent widget tree that Xilem's reactive layer updates. Masonry is framework-agnostic — it manages the widget tree, event passes, layout, and paint; Xilem is just one client.

State is an arbitrary `'static` Rust type. No `Data` trait, no cloning requirement. `memoize` prunes subtrees whose `PartialEq` key has not changed, providing the coarse-grained optimization Flutter's `const` constructors and `shouldRebuild` provide.

The `lens` / `map_state` adapters zoom into subfields of app state for child components, replacing Druid's confusing `Lens` trait with ordinary closures and monomorphization.

### What Xilem got right

- **View as value type, not observer.** No shared mutable state, no observer registration, no `Arc<Mutex<T>>` for reactivity. The rebuild diff is driven by structural comparison of value types. This is fundamentally compatible with Rust's borrow checker.
- **`rebuild()` is the correct reconciliation primitive.** It matches Flutter's `Element.update(newWidget)` semantics 1:1. Both receive the old description and the new description, mutate the existing retained object, and recurse. This is the right shape for FLUI's element reconciliation.
- **`ViewState` (persistent, per-view-node state across rebuilds) = React hooks.** This is how Xilem handles cursor position in a text field — state that must survive a rebuild but is not part of the application model. Flutter's `State` object serves the same role.
- **Memoize is the right coarse-grained optimization.** Rather than signals, the optimization primitive is "skip this subtree if the input has not changed by `PartialEq`." Simpler to reason about, no subscription graph to manage.

### Hard problems Xilem hit

- **Eagerly evaluated view tree is expensive at scale.** Even though `View` objects are cheap, generating the entire tree every cycle adds up for large UIs. Xilem's `memoize` addresses this but requires the developer to think about placement. Floem's signal-driven approach avoids this by only running closures whose dependencies changed. (Inference: Xilem 0.4's ARCHITECTURE.md explicitly acknowledges this.)
- **Type monomorphization explosion.** Xilem's fully typed view tree generates a unique type per subtree shape. This is correct and fast but can cause compile-time and binary-size blowup for deep, dynamic trees. `AnyView` is the escape hatch but adds overhead.
- **`'static` closures for callbacks.** Every event handler must be `'static`, which means closures must capture owned data or use `Arc`. This is the standard Rust event-handler friction; Xilem doesn't solve it, it just makes the constraint visible.
- **The backend was in poor shape in 2024.** The Linebender 2024 roadmap post explicitly said "Xilem's native backend is in a poor state" — entire modules broken, TODOs without issues, Druid documentation references. The reactive layer was well-designed; the platform plumbing was not. FLUI's investment in `flui-platform` is the right priority order.

### Relevance to FLUI

- **`View::rebuild(prev, element)` is a direct port target.** FLUI's element reconciler should implement this exact signature. The sibling agent's internal audit should be validated against Xilem's battle-tested precedent.
- **`memoize` = `shouldRebuild` in Flutter terms.** FLUI should implement this as `View::can_update(&self, prev: &Self) -> bool` — Flutter's semantics, Xilem's name is just a different surface.
- **`ViewState` as the hook equivalent.** FLUI's `State` object already covers this for stateful widgets. The design is validated.
- **Do not eagerly generate the whole view tree without escape hatches.** FLUI's view generation must support subtree-skipping from the start, not as a later optimization.

---

## 3. Druid post-mortem

### Why it was retired (stated reasons, primary sources)

Raph Levien published the explicit list in "Xilem: an architecture for UI in Rust" (2022):

> "There is a big difference between creating static widget hierarchies and dynamically updating them. The app data must have a `Data` bound, which implies cloning and equality testing. Interior mutability is effectively forbidden. The 'lens' mechanism is confusing and it is not easy to implement complex binding patterns. We never figured out how to integrate async in a compelling way. There is an environment mechanism but it is not efficient and doesn't support fine-grained change propagation."

Colin Rofls's "Some reflections on the Druid architecture" added the operational diagnosis:

- `Data` flowing top-down means **anything can mutate anywhere, in any order**. Widgets cannot assume state seen during `update` is the same as state seen during `event`, because another widget earlier in the call chain may have mutated it.
- Ownership of "widget-private" state (e.g., text field cursor position) is awkward — Druid's model says all state belongs to the top-level `Data`, but a text editor has internal state that has no natural home in the application model.
- Lenses are functional (Haskell-style) and opaque to most Rust developers. Composition is non-obvious.

The `druid` GitHub issue #1945 (2021 roadmap) confirms Colin Rofls's own concern: "once an application gets large, `Data` becomes hard to reason about."

### What Druid got right

- **Event flow is synchronous and top-down.** This survived into Xilem as "mutable access to app state at each node during event dispatch." Flutter does the same thing: events travel down the tree, each node gets `&mut State`.
- **Widget-as-value (not widget-as-object).** Druid widgets are plain structs; the framework holds the mutable state. This is the right instinct.

### Regrets

- `Data: Clone + PartialEq` is the single biggest mistake. It forces application state into an artificial shape.
- Lenses as the composition mechanism: too abstract, insufficient documentation, no macro that handles all real-world cases.
- Threading mutable context through all widget methods precluded parallelism (Levien, 2022).

### Relevance to FLUI

- **FLUI's `View` must not require `Clone + PartialEq` on the application state type.** The `InheritedView` TypeId lookup window is the correct minimal runtime-reflection surface.
- **The widget-private-state problem is Flutter's `State` object.** FLUI already ports this correctly. Do not conflate widget-private state with application state.
- **Lenses → `map_state` closures.** If FLUI needs a zooming mechanism for sub-component composition, use closures that take `&AppState -> &SubState`, not a trait-based lens system.

---

## 4. Iced

### Architecture model

Pure Elm (MVU): `Application` has `State`, `Message` enum, `update(state, message) -> Command`, and `view(state) -> Element`. The entire widget tree is rebuilt on every `view` call. Iced diffs the new tree against the previous one to minimize widget mutations (similar to Xilem). Commands are the async bridge. No retained element state beyond what the diff preserves.

`Command` was renamed to `Task` in Iced 0.13 (2024), signaling the ongoing evolution.

### What Iced got right

- **Simplicity.** Elm is the simplest possible reactive model. For small-to-medium applications it is ergonomic and predictable.
- **Unidirectional flow.** No surprise mutations; `update` is the single mutation point.

### Hard problems and regrets

- **Global `Message` enum becomes a monolith.** Every user interaction is one variant in one enum for the entire application. Nesting enums for sub-screens works but requires manual forwarding. At scale this becomes mechanical boilerplate.
- **`Component` trait was deprecated** because encapsulated state "hampers the use of a single source of truth" (Iced docs). The framework doubled down on Elm rather than introducing retained state. This is a principled decision but it means local widget state (e.g., a dropdown's open/closed state) must live in the application model, which is architecturally wrong for ephemeral view state.
- **Widget continuity lost on tree change.** Iced rebuilds the full widget tree on every `view` call. A text input losing focus when the tree structure changes around it is a known bug class. `keyed_column!` was added as a mitigation. Flutter solves this with keys and element reconciliation; Iced reconstructs the tree.
- **No retained layout cache.** Because the tree is rebuilt each cycle, layout is recomputed from scratch. FLUI's retained render tree with per-object `needs_layout` dirty bits is a strict improvement.

### Relevance to FLUI

- **Do not adopt Iced's approach.** The global `Message` enum and full-rebuild `view` call do not fit a Flutter-parity port. Flutter's `setState` is widget-local; Iced's `Message` is application-global. These are fundamentally different.
- **The deprecated `Component` is a cautionary lesson:** attempting to add local retained state to an Elm architecture is awkward enough that Iced removed the feature rather than fix it. FLUI's `StatefulView`/`State` pairing is the correct model for local state.

---

## 5. Makepad

### Architecture model

"Retained-geometry, immediate-event": a persistent widget tree in a `WidgetTree`, with events processed immediately (not deferred). The `LiveDesign` DSL (now `script_mod!` macro) defines UI structure and supports live-reload. Widgets call `cx.redraw()` to set a `paint_dirty` flag; no diff happens — the next frame rerenders. The `Cx` context is a platform abstraction object threaded through all widget methods.

### What Makepad got right

- **Live-reload as a first-class feature.** The architecture was designed for live-reload from the start, not grafted on later. This matches FLUI's `flui-hot-reload` mandate.
- **Retained widget tree for performance.** The tree persists; only dirty widgets repaint. Similar to Flutter's `needsPaint` flag.
- **Custom shaders per widget.** Makepad provides a shader DSL per widget, enabling highly customized visual behavior without framework interference.

### Hard problems

- **DSL churn.** The transition from `live_design!` compile-time macros to `script_mod!` runtime evaluation was breaking and confusing. A DSL that evolves incompatibly is a user-trust problem.
- **`Cx` threading.** Threading a mutable context through all widget methods is the same problem Druid had. It enforces single-threaded execution.
- **Limited ecosystem.** Makepad is primarily used for the Makepad Studio IDE; external contributor base is small.

### Relevance to FLUI

- **Hot-reload must be an explicit architectural constraint, not a later add.** Makepad's precedent confirms FLUI's `flui-hot-reload` crate must define its contract (what is reloadable, what requires restart) before the widget catalog is built. If the hot-reload boundary is not defined early, it will be incompatible with the widget tree.
- **DSL vs macros: avoid a custom DSL.** FLUI uses Rust builder-chain APIs; this is correct. A DSL adds a parser/compiler surface that breaks IDEs and requires migration on every syntax change.

---

## 6. Floem

### Architecture model

Floem (Lapce project) is a **signals-first retained-mode** framework. The view tree is built **once** (not rebuilt each cycle). Signals (`RwSignal<T>`, inspired by Leptos) drive targeted updates: when a signal changes, only the closures that read it are re-executed, sending update messages to specific `ViewId`s. The `View::update()` method on the affected node processes the change without traversing the tree from root. No diff, no full rebuild.

This is the **Solid.js / Leptos model** applied to a native Rust retained-mode GUI.

### What Floem got right

- **View tree constructed once is a genuine performance win.** For apps with large static structure and small dynamic regions, this avoids regenerating the full tree description each event. Floem's README is explicit: "The view tree is constructed only once, safeguarding you from accidentally creating a bottleneck."
- **Fine-grained updates without a diff.** Signal subscription means only the affected label re-renders, not the subtree. This is O(1) invalidation for a single signal change.
- **Practical.** Floem is used in Lapce (a real code editor); it works at production scale.

### Hard problems

- **Signal ownership and `move` closures.** Every reactive closure must `move` its captured signals. `RwSignal` is `Copy` (backed by a generational arena), which alleviates some pain, but the discipline is different from Flutter's `setState`.
- **View constructed once means dynamic structure is harder.** Adding or removing a child widget requires different mechanisms than updating a property. Floem handles this with `dyn_view` and dynamic containers, but it is a distinct concept from updating a signal.
- **No reconciliation.** Because the tree is not rebuilt, there is no key-based element reuse. Dynamic lists require virtual list primitives; there is no free reconciliation of arbitrary child lists.
- **Debugging reactivity.** When a UI element does not update, the developer must trace signal subscriptions. Floem's docs explicitly warn: "if you ever encounter a bug where something doesn't get updated, the first thing you'll need to check is 'is it a closure?'"

### Relevance to FLUI

- **Floem's signal model and FLUI's three-tree model are structurally incompatible.** Flutter's `Element.rebuild()` is called top-down from the dirty element root; Floem's signals fire bottom-up to specific view nodes. Mixing them would require two invalidation mechanisms. (See Reactivity section for the full analysis.)
- **The single-build view tree has an important ergonomic property** FLUI should be aware of: if `View::build()` is expensive and called on every cycle, developers will want subtree-skipping (`memoize`). FLUI must provide this from the start.

---

## 7. Dioxus

### Architecture model

Dioxus is a VDOM-based framework inspired by React. Components are functions; state lives in `Signal<T>` (since 0.5, using `GenerationalBox<T>` for `Copy` ergonomics). A `VirtualDom` tracks dirty components and re-renders only those. Renderers implement `WriteMutations` to apply the diff to a target platform (web DOM, desktop webview, native).

The 0.1–0.4 architecture used lifetimes to relax borrow rules. 0.5+ switched to `Copy` signals backed by a generational arena, eliminating lifetime pain at the cost of indirection.

### What Dioxus got right

- **`Copy` signals eliminate the closure-capture problem.** `Signal<T>` is `Copy`; you move it into event handlers and async tasks without cloning or `Arc`. This is the cleanest solution to the "closures can't capture `&mut state`" problem in Rust.
- **Platform-agnostic VDOM.** Dioxus can target web, desktop (via webview), native, TUI. The `WriteMutations` abstraction is clean.

### Hard problems

- **VDOM diff overhead.** Every component re-render produces a VDOM that must be diffed. For fine-grained updates, signal subscription short-circuits the diff; but for coarser updates the full VDOM path runs.
- **Webview rendering quality.** Dioxus desktop uses a webview (WebView2/WebKitGTK), which means rendering is HTML/CSS, not GPU-native. For UI quality matching Flutter, this is insufficient.
- **Signal hook rules.** Hooks must be called in the same order every render — the React constraint. This is a source of subtle bugs.

### Relevance to FLUI

- **`Copy` signal via generational arena is worth noting** as the cleanest Rust solution to signal ergonomics if FLUI ever adopts signals. Dioxus's `GenerationalBox` is the prior art.
- **VDOM+renderer split is architecturally close to View→Element→Render** but serves a different target. FLUI's retained Element and Render trees are more Flutter-faithful and do not need a VDOM intermediate.

---

## 8. egui

### Architecture model

Immediate mode: every frame the application re-runs its UI code top-to-bottom. No retained widget tree. State is stored externally by the application or in `egui`'s `Memory` keyed by stable `Id`. Layout is a known challenge: sizing and positioning require the content size, which is only known after layout. egui handles this with multi-pass rendering (since 0.29: `UiBuilder::sizing_pass`, `Context::request_discard`).

### Relevance to FLUI

egui confirms what Raph Levien wrote: immediate mode is fundamentally inadequate for accessibility (which requires stable widget identity and retained state) and complex layout (first-frame jitter, multi-pass cost). FLUI is right to build a retained-mode three-tree. **Do not consider egui-style immediate mode as a future simplification path.**

The one genuine egui lesson: **make the testing story simple.** egui's `TestCtx` and low setup cost make it easy to write UI tests. FLUI's `HeadlessPlatform` and test infrastructure should match this ergonomic bar.

---

## 9. Slint

### Architecture model

Slint uses a compiled DSL (`.slint` files) with two-way property bindings. The property system tracks dependencies via an intrusive doubly-linked list. Bindings are lazy: a dirty flag is set, and the value is recomputed on next read. This is push-dirty / pull-compute — the same model as Solid.js computed signals.

### What Slint got right

- **Compiled DSL enables deep compile-time checking** of binding expressions (purity enforcement) and layout constraints.
- **Two-way bindings with property aliasing** (`<=>`) are clean for form widgets.

### Hard problems

- **Imperative writes break bindings.** `foo.bar = 42;` silently breaks a `foo.bar` binding. The framework documents this but it is a footgun.
- **Rust API is secondary.** Slint's primary audience is the DSL; the Rust API is a bridge. This inverts FLUI's value proposition (Rust is first-class).
- **Binding purity constraint.** Binding expressions must not mutate observable state, enforced by the compiler. This is correct but requires a bespoke analysis pass.

### Relevance to FLUI

- **DSL-first is the wrong direction for FLUI.** FLUI's target user explicitly rejects HTML/CSS mental models. A `.flui` DSL would require the same compiler tooling Slint built and would make Rust tooling (rust-analyzer, cargo) second-class.
- **Property dependency tracking is a valid optimization** for `InheritedView`: Flutter's `InheritedWidget` already uses a hash table of dependent elements per `BuildContext` to avoid O(N²) parent-chain walks. FLUI's `TypeId` registry is the Rust analog. This is correct.

---

## 10. Vello

### Architecture model

Vello is a compute-shader-centric 2D renderer. All path rasterization happens in GPU compute shaders via prefix-sum parallelism, avoiding CPU tessellation and intermediate textures. The pipeline: scene encoding → path flattening → binning → coarse rasterization → fine rasterization (antialiased). 177 fps on `paris-30k` (M1 Max, 1600px square). Requires WebGPU-capable compute shader support.

Three backends exist: GPU-only (production), CPU-only (for testing, incomplete), and `vello_hybrid` (CPU path processing + GPU compositing; targets WebGL2 and resource-constrained devices, merged March 2025).

A next-generation "sparse strip" renderer (issue #670, Levien 2024) is under research: promises better performance and modular integration into other systems.

### Lyon tessellation vs Vello compute: the tradeoffs

| Dimension | Lyon tessellation (FLUI current) | Vello compute |
|---|---|---|
| GPU requirement | Any GPU with vertex/fragment shaders | Compute shader support (WebGPU level) |
| CPU cost | Tessellation on CPU, upload triangles | Scene encoding only; rasterization on GPU |
| Dynamic scenes | CPU tessellate on every path change | Re-encode scene; GPU handles rasterization |
| Masking/blending | Multiple draw calls, intermediate textures | Single compute pass, vector-register compositing |
| Hardware coverage | Maximum (includes integrated GPUs, WASM) | Limited (no WebGL2 without hybrid fallback) |
| Maintenance | Lyon is mature and stable | Vello is in active development, API unstable |
| Integration cost | Drop-in wgpu draw calls | Requires wgpu context setup, not trivial |

### Relevance to FLUI

- **Lyon is the correct choice for the current phase.** FLUI's platform integration is incomplete; switching renderers during `flui-platform` MVP would be premature. Lyon is battle-tested, works on all wgpu targets, and requires no compute shader support.
- **Plan a renderer abstraction seam.** The `flui-engine` crate should define a `SceneRecorder` / `RasterBackend` abstraction that allows swapping Lyon for Vello later. The existing wgpu 25.x dependency and the `flui-painting` `DisplayList` are the right shape. Do not couple the painting model to Lyon tessellation specifics.
- **Watch Vello's sparse-strip work.** If sparse strips land with a stable API and `vello_hybrid` closes the WebGL2 coverage gap, the migration argument strengthens. Re-evaluate at FLUI 0.3 when the render pipeline is stable.
- **`vello_hybrid` is the risk-mitigated migration path** when the time comes: CPU path processing (compatible with FLUI's existing painting model) + GPU fine rasterization. Not requiring full compute shaders.

---

## Cross-Cutting Hard Problems

### 1. Reactive state under ownership/borrow checking

This is **the central problem of Rust UI**. Every framework has solved it differently:

- **Druid:** `Data: Clone + PartialEq` — sidestep by cloning. Failed at scale.
- **GPUI:** App-owns-all + lease pattern — sidestep by temporarily moving state to the stack. Works, but single-threaded.
- **Xilem:** View is a value; state is threaded as `&mut AppState` through the event dispatch. No shared mutable state. Correct.
- **Floem/Dioxus:** `Copy` signals backed by generational arenas — sidestep by making state handles `Copy`. Works for ergonomics; adds indirection.
- **Iced:** Global `Message` enum — sidestep by never holding mutable references in widgets. Works for simple apps; fails at scale.

Flutter's Dart solution (garbage collection, no move semantics) does not translate. FLUI's approach — `View::build()` receives `&BuildContext` (read-only), events receive `&mut State` through the element chain — is the Xilem-validated path.

### 2. Closures/callbacks capturing state

Every Rust UI framework suffers from `move` closure pain: `on_click(move || { ... })` must own its captures. The three mitigations found in practice:

1. **`Copy` signal handles (Dioxus, Floem):** signal is a small generational pointer; `move` is free.
2. **Arc-wrapped shared state:** `Arc<Mutex<T>>` in the closure. Works but adds overhead and risks deadlock.
3. **ID-based dispatch (Xilem, Flutter):** callbacks do not capture state; they dispatch an action/message by ID, and the framework routes the action to the correct handler with `&mut AppState` available. This is the cleanest model.

FLUI's event dispatching should follow the ID-path model (Xilem-validated, Flutter-aligned): events carry a path down the element tree, each node gets `&mut State`, no closure captures mutable state.

### 3. Retained widget tree + diffing/reconciliation in Rust

The hard part is not the diff algorithm (Flutter's O(N) linear reconciliation is well-understood and directly portable). The hard part is **who owns the children during reconciliation**.

Xilem's RFC (`0001-masonry-backend.md`) documents the "you own your children" problem in Xilem's pre-Masonry architecture: `Vec<Pod<Widget>>` owned by each container made it impossible to iterate the full widget tree for the inspector or route a focus event without traversing the ownership chain.

The solution (adopted in Masonry) is **library-owns-widgets**: widgets are stored in a Slab/SlotMap; containers hold keys. FLUI's Slab-based element store with `ElementId = NonZeroUsize` and the `+1`/`-1` offset convention is already the correct shape. This is validated.

### 4. Threading a context through `build` without lifetime hell

The core tension: `build(cx: &mut Cx)` needs to hand `cx` to child builds, but Rust's borrow checker prevents lending the same `&mut` to two places simultaneously.

Three patterns observed:

- **Thread `&mut Cx` through parameters** (Druid, Makepad): single-threaded, explicit, verbose.
- **Thread-local context** (React, egui `Context`): no parameter, implicit, works in single-threaded setting, but `RefCell`-borrow-panics are possible under reentrancy.
- **App-owns-all + lease** (GPUI): explicit, safe, single-threaded.

FLUI's `BuildContext` with `Arc<RwLock<ElementTree>>` and `Arc<RwLock<BuildOwner>>` (noted as "latent friction" in `crates/flui-view/FRICTION.md`) is a pragmatic choice. The friction is real: `RwLock` on these infrastructural objects is allowed per `PORT.md`'s lock decision table, but long-term the correct shape is to own the element tree behind a `&mut ElementTree` that the build phase holds exclusively. This avoids lock overhead on the hot path. Flag as a future Outstanding Refactor.

### 5. Text / IME / accessibility

Every surveyed framework identifies these as the hardest non-architecture problems:

- **Text:** FLUI correctly uses `cosmic-text` (fontdb + rustybuzz + swash). This is the validated Rust-native stack (used in Iced, Floem, COSMIC desktop). Known gaps: Arabic ligature constructions that require runtime lookup rule building; `avar2` variable font interpolation. Defer both; they are font-shaping edge cases.
- **IME:** IME requires stable widget identity across frames (so the input method can track the cursor). FLUI's persistent element tree with `ElementId` is the correct foundation. The specific IME protocol per platform (Win32 `WM_IME_*`, macOS `NSInputClient`, Wayland `text-input-v3`) must be implemented in `flui-platform`. Note: egui's IME bugs were cited in the 2025 Rust GUI survey as a showstopper for some users.
- **Accessibility:** AccessKit is the de-facto Rust accessibility bridge (used by Xilem/Masonry, egui, kas-gui). FLUI's `flui-semantics` maps to AccessKit's model. The key architectural requirement: stable `AccessKit` node IDs tied to `SemanticsId` across frames. FLUI's NonZeroUsize ID system already provides this. Integration is pending but the foundation is correct.

### 6. Incremental layout and invalidation

Flutter's layout protocol (constraints flow down, sizes flow up, `needs_layout` dirty bits per node) is the right model and FLUI already ports it. The key lesson from the ecosystem:

- **Immediate-mode frameworks pay full re-layout every frame** (egui, Makepad for fully dynamic content). Retained-mode with dirty bits avoids this.
- **Taffy (flexbox) is a reasonable alternative** if Flutter's constraint protocol is too expensive to port fully. GPUI and Floem use Taffy. However, Flutter's constraint protocol and Taffy give different results for the same widget tree; choosing Taffy would mean FLUI's layout behavior diverges from Flutter's. Given "behavior loyal, structure Rust-native," stay with Flutter's constraint protocol.
- **The hardest case is virtualized lists.** Flutter's `SliverList` with lazy element creation is the gold standard. Xilem's roadmap explicitly mentions virtual lists as a hard problem. Floem's `virtual_list` is a production implementation worth studying. Defer to a dedicated milestone.

### 7. Hot-reload

Makepad designed for it from the start and it works. Xilem's 2024 roadmap identified hot-reload as a goal (not yet delivered as of 0.4). Flutter's hot-reload is the user expectation.

The architectural constraint: **hot-reload requires that widget state survives a widget-code reload.** This means state cannot live in the widget struct itself if the struct type changes across reload. State must be either in the element tree (keyed by element type + position, not by widget struct identity) or in an external store keyed by a stable ID. Flutter achieves this because `State` is owned by the `Element`, not the `Widget`; FLUI's `State` owned by the `Element` is the same correct design.

FLUI's `flui-hot-reload` crate must define the boundary: what constitutes a "hot-reload-safe change" vs. a "cold restart." Minimum viable: reload changes to `View::build()` return values. Non-viable without cold restart: changes to `State` struct layout.

---

## Recommendations for FLUI (Ranked)

### R1: Confirm and protect the three-tree architecture (highest priority)

The ecosystem has validated this. Xilem is converging on it (Masonry = retained layer, Xilem = reactive layer). GPUI's single-tree-dropped-per-frame model is productive for a code editor but inadequate for a full widget toolkit with accessibility, IME, and layout caching. Do not simplify.

**Source:** Xilem RFC 0001; Flutter inside-flutter.md; GPUI element.rs module doc.

### R2: `setState`/Element-rebuild is the correct reactivity model — with one modification

**The sibling agent is right, with a qualification.** Keep Flutter's `setState` + `InheritedView` as the canonical reactivity model. Do not introduce signals into the widget catalog's dependency graph. The reasons:

1. Signals require subscription management (effects, memos, scopes) as a runtime system running alongside the three-tree. Two invalidation mechanisms in one framework are a maintenance and correctness hazard.
2. Flutter's `setState` + dirty-element list + O(N) linear reconciliation is a proven system at production scale. The Rust port of the algorithm (mark dirty → `BuildOwner::flush_dirty_elements()` → rebuild dirty subtrees in depth order) is straightforward.
3. `InheritedView` (= `InheritedWidget`) provides the "scoped reactive dependency" pattern without signals: a widget rebuilds when the nearest ancestor `InheritedView` changes, tracked via the element's dependency set. This covers 90% of signal use cases in practice (theme, locale, media query, auth state).
4. The Druid post-mortem shows that mutable access to app state during event dispatch (the `setState` shape) is the right primitive. Signals require the observer be registered before the event fires; `setState` makes no such demand.

**The qualification:** `memoize` must be implemented from the start (call it `View::can_update` or expose a `Memoize` combinator). Xilem learned that without subtree-skipping, eagerly building the full view tree on every cycle creates performance cliffs. FLUI's `View::build()` is called by the framework; the framework must be able to skip calling it if the view's inputs have not changed. This is `shouldRebuild` in Flutter's internal `Element.updateChild()`.

**Source:** Raph Levien "Xilem architecture" (2022); Flutter inside-flutter.md "build phase" and "reactive paradigm"; Floem docs on why single-build + signals avoids the full-rebuild cost; Druid post-mortem.

### R3: Implement the lease pattern in `BuildContext` (medium priority)

GPUI's documented solution to "need `&mut widget_state` and `&mut Cx` simultaneously" is directly applicable to `flui-view`'s `BuildContext`. The current `Arc<RwLock<ElementTree>>` wrapper is functional but adds lock overhead on the hot path. The endgame: `BuildPhase` holds `&mut ElementTree` exclusively; `BuildContext` is a view into that exclusive reference; no runtime locking needed. This aligns with Refusal Trigger 1 (no `RwLock` in the build/layout/paint hot path) and with the "sync hot path" clause.

**Source:** GPUI `app/context.rs` lease pattern; PORT.md lock decision table (BuildContext locks noted as "latent friction").

### R4: Plan a renderer abstraction seam now (medium priority)

FLUI uses lyon tessellation today. Vello's compute-shader approach is maturing and `vello_hybrid` reduces the hardware requirement. Rather than coupling `flui-painting` to lyon specifics, define a `PathRasterBackend` trait in `flui-engine` with `record_path(path, transform, fill)` as the interface. Lyon implements it now; Vello implements it later. The `DisplayList` recording in `flui-painting` is already a good seam.

**Source:** Vello README; vello_hybrid PR #831; existing `docs/research/2026-03-31-gpu-tessellation.md`.

### R5: AccessKit integration before the widget catalog ships (medium priority)

Every framework that skipped accessibility and added it later paid a large retrofit cost (egui's AccessKit integration was complex; Xilem's Masonry built it in from the start). FLUI's `flui-semantics` must integrate with AccessKit via the `SemanticsId` → `AccessKit::NodeId` bridge before the first public widget is shipped. If a widget ships without an accessibility role, it becomes a breaking-change target later.

**Source:** Xilem/Masonry AccessKit integration; egui's experimental screen reader; kas-gui accessibility checklist.

### R6: Define hot-reload boundary in `flui-hot-reload` before widget catalog (medium priority)

Makepad's lesson: hot-reload designed in from the start works. Retrofitted hot-reload requires compromises. FLUI's `State`-owned-by-`Element` design already gives the correct foundation. Write the spec: what changes hot-reload can handle (view tree shape, style values, `build()` return values) vs. what requires cold restart (`State` struct layout changes, crate API changes).

**Source:** Makepad architecture; Xilem 2024 roadmap (hot-reload as primary 2024 goal, still undelivered as of 0.4).

### R7: Invest in the text/IME platform layer early (lower priority, high eventual impact)

cosmic-text is the right stack. The platform-side IME hooks (Win32 `WM_IME_COMPOSITION`, macOS `NSInputClient`, Wayland `text-input-v3`) must land in `flui-platform` before the `TextInput` widget ships. The 2025 Rust GUI survey cites IME as a user-facing showstopper. GPUI (Zed) has working IME on all three platforms — consult `.gpui/src/platform/mac/text_system.rs`, `.gpui/src/platform/windows/direct_write.rs`, and `.gpui/src/platform/linux/text_system.rs` for implementation reference.

**Source:** GPUI vendored platform source; 2025 Rust GUI survey; "Why Font Rendering in Rust Is Harder Than It Looks" (2026-05-22).

---

## The Reactivity Model Decision

### The options

**Option A: Pure Flutter `setState`/Element-rebuild (sibling agent recommendation)**

Mechanism: `State::set_state()` marks the element dirty, `BuildOwner::flush_dirty_elements()` rebuilds dirty subtrees in depth-first order during the next build phase. `InheritedView` provides scoped reactive dependency: elements register as dependents of the nearest ancestor `InheritedView`; when the `InheritedView` changes, its dependents are marked dirty. `memoize` / `View::can_update()` skips subtrees whose inputs have not changed.

Tradeoffs:
- Pro: 1:1 Flutter behavior port. Behavior-loyal, well-understood algorithm, direct test suite mapping.
- Pro: Single invalidation mechanism. No signals runtime alongside the three-tree.
- Pro: `InheritedView` covers cross-cutting state (theme, locale, auth) without signals.
- Pro: `setState` is widget-local; only the subtree rooted at the dirty element rebuilds.
- Con: No fine-grained sub-widget invalidation without `memoize` placement. A widget whose `build()` returns a large subtree that partially depends on one value must use `memoize` to avoid rebuilding the whole subtree.
- Con: App-level state (`setState` inside a root `State`) triggers a top-down rebuild of that subtree. For deeply nested dynamic values, `InheritedView` is needed to avoid O(depth) rebuilds.

**Option B: Signals-first (Floem/Dioxus model)**

Mechanism: Signals replace `setState`. Each signal registers dependent closures; changes trigger only the subscribed closures. The view tree is built once; updates are surgical.

Tradeoffs:
- Pro: Fine-grained updates without `memoize` placement discipline.
- Pro: View tree built once avoids repeated `build()` call overhead.
- Con: Two invalidation systems if combined with Flutter's build phase (signals fire immediately; build phase is batched). Inconsistent semantics.
- Con: Breaks behavior loyalty. Flutter's `setState` is a batched, depth-ordered rebuild; signals are immediate, subscription-ordered. The algorithms are different.
- Con: Signal subscription management (effects, memos, scopes, cleanup on teardown) is a significant runtime surface. `flui-reactivity` is currently disabled — for good reason; this is premature complexity.
- Con: "Build the tree once" means dynamic child-list changes (add/remove children from a `Column`) require explicit dynamic container primitives rather than the natural `build()` return value. Flutter's `build()` returning a new child list and the reconciler computing the diff is simpler.
- Con: The debugging model breaks down: "something doesn't update? Check if it's a closure" (Floem docs) is an unfamiliar diagnostic for developers coming from Flutter.

**Option C: Hybrid (keep `setState`, add signals as an opt-in primitive for specific use cases)**

Mechanism: `setState` and `InheritedView` are canonical. A separate `Signal<T>` type (backed by `flui-reactivity`) is available for application state that truly benefits from fine-grained subscription (e.g., a live data stream updating a chart). Signals are not allowed in the widget catalog's dependency graph (FLUI would enforce this through Refusal Trigger extension).

Tradeoffs:
- Pro: Covers the genuine use case where signals shine (real-time data, animation state).
- Con: Two state models that developers must learn to choose between. Increases cognitive surface.
- Con: `flui-reactivity` is disabled; building it correctly is a significant project. The risk of premature design is high.

### Recommendation

**Adopt Option A. The sibling agent is correct.**

Reasoning:

1. **FLUI's mandate is "behavior loyal."** Flutter's `setState`/`InheritedWidget` is behavior, not implementation. The Rust port must preserve this behavior. Option B would produce a framework that behaves like Floem, not Flutter — different update order, different developer mental model, different debugging approach.

2. **The ecosystem convergence is toward Option A for Flutter-class frameworks.** Xilem (the closest architectural analog to FLUI) uses `rebuild()` + `memoize`, not signals. This is Raph Levien's considered position after building Druid (observer pattern), Crochet (immediate mode), and Xilem (value-type view + rebuild diff). His conclusion after all three experiments: threaded `&mut state` + rebuild diff is the correct model for Rust retained-mode UI.

3. **Signals impose a runtime cost that the hot path cannot afford.** FLUI's Refusal Trigger 3 bans `async fn` on `build`/`layout`/`paint`. The same reasoning applies to signal subscription: a signal firing during `paint` would be an async-style notification in the sync hot path. Option A's `setState` + build phase keeps all mutation batched and explicit.

4. **`InheritedView` covers the primary signals use case.** When a developer wants "all widgets below this point to reactively respond to a value change," `InheritedView` is the answer. It is O(1) to notify (the dependent element set is stored on the `InheritedElement`), depth-ordered (Flutter's dirty-element list is sorted by depth), and behavior-loyal.

5. **`memoize` is sufficient for performance.** The Xilem experience shows that `memoize` + `PartialEq` provides adequate subtree skipping. The cost of occasional extra `build()` calls on unchanged subtrees is bounded by the O(N) linear reconciler. For the widget catalog (Material/Cupertino), the benchmark is Flutter itself — and Flutter uses `setState` exclusively in its widget library.

**Disagree with adding signals to `flui-reactivity` in the near term.** Keep `flui-reactivity` disabled until the three-tree is fully operational and the widget catalog has non-trivial coverage. If signals are added later, they must be constrained to application-layer state (outside the widget catalog) and explicitly excluded from the build/layout/paint path via a new Refusal Trigger (precedent: Trigger 3 for async, Trigger 4 for Mutex on dirty lists).

---

## Most Dangerous Trap to Avoid

**The `Data: Clone + PartialEq` trap.** Druid made application state artificially constrained. The immediate-mode-over-retained-mode hybrid (Crochet) produced state tearing. The `Component` encapsulated-state experiment in Iced was removed. In each case, the framework made a structural choice about state shape early, and that choice became load-bearing — expensive to change and limiting to users.

FLUI's mandate — application state is `'static`, no constraints, `setState` is the mutation primitive — is the correct position validated by the full ecosystem survey. Hold this line. Any proposal to add a trait bound to `AppState` or to make `View` carry a `Signal<T>` in the catalog should be rejected with reference to this post-mortem chain.

---

## Sources

| Source | URL / Path | Description |
|---|---|---|
| GPUI app context | `.gpui/src/app/context.rs` | Entity model, lease pattern, observe/subscribe |
| GPUI element | `.gpui/src/element.rs` | Per-frame element drop, three-pass layout |
| GPUI app | `.gpui/src/app.rs` | AppCell, RefCell, single-thread design |
| Zed GPUI blog | https://zed.dev/blog/gpui-ownership | Ownership, lease pattern, flush_effects |
| Raph Levien Xilem blog | https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html | Three-tree design, Druid limitations list |
| Raph Levien "next dozen GUIs" | https://raphlinus.github.io/rust/gui/2022/07/15/next-dozen-guis.html | Crochet post-mortem, state tearing |
| Raph Levien "principled reactive UI" | https://raphlinus.github.io/rust/druid/2020/09/25/principled-reactive-ui.html | Druid lens problems, incremental computation |
| Colin Rofls Druid reflections | http://www.cmyr.net/blog/druid-architecture.html | Data trait failure modes, widget-private state |
| Druid issue #1945 | https://github.com/linebender/druid/issues/1945 | "Data becomes hard to reason about at scale" |
| Xilem ARCHITECTURE.md | https://docs.rs/crate/xilem/latest/source/ARCHITECTURE.md | View tree lifecycle, memoize, app_logic |
| Xilem RFC 0001 | https://github.com/linebender/rfcs/blob/main/rfcs/0001-masonry-backend.md | Masonry backend rationale, "you own your children" problem |
| Linebender 2024 roadmap | https://linebender.org/blog/xilem-backend-roadmap/ | Backend state, hot-reload goal, widgets-in-slotmap |
| Floem README | https://github.com/lapce/floem/blob/main/README.md | Signal-first, single-build view tree |
| Floem docs | https://docs.floem.dev/ | Reactivity model, closure requirement |
| Dioxus signals notes | https://github.com/DioxusLabs/dioxus/blob/main/notes/architecture/04-SIGNALS.md | GenerationalBox, Copy signal ergonomics |
| Vello README | https://github.com/linebender/vello/blob/main/README.md | Compute-shader rasterization, 177fps benchmark |
| Vello hybrid PR | https://github.com/linebender/vello/pull/831 | CPU+GPU hybrid renderer for WebGL2 |
| Flutter inside-flutter.md | https://github.com/flutter/website/blob/main/src/content/resources/inside-flutter.md | Linear reconciliation, InheritedWidget hash table, setState dirty list |
| Font rendering in Rust | https://dev.to/kent-tokyo/why-font-rendering-in-rust-is-harder-than-it-looks-9db | cosmic-text stack, known gaps |
| 2025 Rust GUI survey | https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html | IME as showstopper, comparative assessment |
| DeepWiki: xilem | deepwiki.com (linebender/xilem) | View trait, AppState model, Druid retirement |
| DeepWiki: iced-rs | deepwiki.com (iced-rs/iced) | Elm architecture limitations, Component deprecation |
| DeepWiki: lapce/floem | deepwiki.com (lapce/floem) | Signal system, UpdaterEffect, lifetime management |
| DeepWiki: linebender/vello | deepwiki.com (linebender/vello) | Compute approach, Lyon comparison, tradeoffs |
| DeepWiki: makepad | deepwiki.com (makepad/makepad) | LiveDesign DSL, retained-geometry model |
| DeepWiki: DioxusLabs/dioxus | deepwiki.com (DioxusLabs/dioxus) | VirtualDom, signal Copy ergonomics |
| DeepWiki: emilk/egui | deepwiki.com (emilk/egui) | Immediate-mode layout problems, multi-pass |
| DeepWiki: slint-ui/slint | deepwiki.com (slint-ui/slint) | Property system, two-way binding, purity |
| DeepWiki: zed-industries/zed | deepwiki.com (zed-industries/zed) | Flat entity store, App ownership |
