# ADR-0393: A development reload applies one of three tiers, chosen from the shape of the edit

- **Status:** Accepted (2026-11-23)
- **Date:** 2026-11-23
- **Supersedes:** [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md)
  §1 and §7 only, in part: that `ReloadEvent` has no restart event and that one arrives with a
  code patcher's record
- **Related:** [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) (the
  hook that reports a reload);
  [ADR-0394](ADR-0394-application-code-reloads-as-an-image-the-element-tree-adopts.md) (the
  proposed mechanism that loads new code, and what guards the memory it reuses);
  [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the rejected patcher, whose structural
  hash is option 3 here); [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the
  runtime applies a tier and names no tool);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (containment of the user
  code a restart runs); [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)
  §7 (`renew`, which a restart calls)
- **Sources:** `flutter/flutter@3.50.1`
  `packages/flutter/lib/src/foundation/binding.dart:742` (`reassembleApplication`),
  `packages/flutter/lib/src/widgets/binding.dart:1715` (`performReassemble`),
  `packages/flutter/lib/src/widgets/framework.dart:1053` (`State.reassemble`), `:3471`
  (`BuildOwner.reassemble`), `:3772` (`Element.reassemble`);
  `packages/flutter_tools/lib/src/run_hot.dart:604` (`_restartFromSources`), `:759` (a rejected
  reload), `:1586` (the advice to restart)

## Context and problem statement

An application author edits code and waits for the running application to show it. What `main`
holds:

| Fact | Evidence |
|---|---|
| The runtime names three tiers: `Reassemble`, `Restart`, `ProcessRestart` | `ReloadTier` in `crates/flui-runtime/src/reload.rs` |
| Only `Reassemble` is built: `Restart` logs that the root remount is not implemented and reassembles | `Presentation::apply_hot_reload` in `crates/flui-runtime/src/presentation.rs` |
| A hook reports `Unchanged` or `Patched`, and the host maps `Patched` to `Reassemble` | `ReloadEvent` in `crates/flui-view/src/dev_reload.rs`; `reload_tier` in `crates/flui-app/src/app/hot_reload.rs` |
| An edit that cannot keep `State` replaces the process: every edit in a project from `flui create`, and an edit to the types crate of one from `flui create --hot-reload` | [`docs/hot-reload.md`](../hot-reload.md), "Getting a hot-reload project" and "Reload Strategies" |
| A process restart after a one-line edit takes about 2.9 s, of which about 0.25 s is the start to the first frame; a worker reload takes 1.5–2.0 s | run of 2026-11-23, generated counter, Linux x86_64, 4 cores |
| Between two builds of one crate with the same `-C metadata`, a type's `TypeId` is unchanged when a field's type changes and when an `async` block keeps more across an `.await`, while its size changes | run of 2026-11-23, rustc 1.100.0 |
| Flutter keeps `State` across a hot reload and rebuilds every element; when the VM rejects a reload the tool advises a hot restart, which starts the program again in the same engine and window | Sources |

Rust has no VM that could give a live object a new layout, so an edit that changes a type cannot
keep the values of that type. Which edits keep `State`, what happens to the others, and who
decides?

## Decision drivers

- Flutter's loop is the model: an edit shows in under a second with `State` kept, and when it
  cannot be kept the tool says so and starts the application again without losing the window.
- An author writes widgets as before: no derive, attribute or registration exists for reload.
- New code never reads a value an older build laid out differently. A slower tier is the price of
  an edit the tool cannot place; a crash in the middle of an edit is not.
- A window, its surface, the GPU context with its pipelines, and the font collection do not depend
  on application code, and recreating them is what a process restart spends its start on.
- The hook names no tool and the runtime names no host (ADR-0311, ADR-0083).
- CI executes Linux only, so each rule is held by a headless test.
- The author learns which tier ran and why.

## Considered options

1. Two tiers, as built: reassemble, and replace the process for everything else.
2. Three tiers, and the tool chooses one from the shape of the edit.
3. Three tiers, and the author's types declare what can be kept: a derive writes a structural hash
   for every retained `View` and `ViewState` (ADR-0094 §3).
4. Per-type tiers: remount only the subtrees whose `State` type changed and keep the rest.

## Decision

Chosen option: 2, because it gives the author Flutter's two fast outcomes without asking them to
mark anything, and because the one party that sees the whole edit, the tool, is the one that
decides.

§1. **A reload applies one of three tiers**, the variants of `flui_runtime::reload::ReloadTier`:

| Tier | What it does | What survives |
|---|---|---|
| `Reassemble` | every element rebuilds with the code now loaded | every `State`, signal, focus, scroll offset and route |
| `Restart` | each UI runtime unmounts its root and mounts the root the code now loaded builds | the process, the event loop, the windows the runner opened, their surfaces, the GPU context and its pipelines, the font collection |
| `ProcessRestart` | the supervisor replaces the process | nothing |

Flutter's counterparts are hot reload, hot restart and a full restart.

§2. **The tool chooses the tier from the shape of the edit.** The *shape* of a crate is its source
with function bodies taken out. `flui run` computes it from the tokens of every `.rs` file of the
crate, as `syn` parses them:

- In the shape: every item with its attributes; signatures, generics and `where` clauses; fields
  and variants; trait and `impl` headers; `const` and `static` initialisers; the body of a
  `const fn`, since a type's layout may be computed from it; every token of a macro invocation in
  item position; and each item declared inside a body, wherever it is nested.
- Outside it: the block of a function, method or closure apart from the items it declares;
  comments and doc attributes; spans and whitespace.

A *reloadable crate* is one whose code the reload mechanism can load again; its record lists them
(ADR-0394 §1).

| The edit, since the build the application runs | Tier |
|---|---|
| The shape of every reloadable crate is unchanged | `Reassemble` |
| The shape of a reloadable crate changed, or a file of one that the build read and the tool does not parse | `Restart` |
| Anything else the build's dependency information names: a manifest, the lock file, a build script, a crate that is not reloadable | `ProcessRestart` |
| The sources do not build | none: the running application is left as it is and the error is shown |

The tool errs upward: what it cannot place takes the higher tier. A reload mechanism may raise the
tier the tool chose and never lowers it. The author may ask for a higher tier than the tool
chose, as Flutter's `R` does, and never a lower one.

§3. **A hook reports the tier and supplies the root.** In `flui_view::dev_reload`:

```rust
/// Exhaustive: a host maps every event to the tier it applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadEvent {
    Unchanged,
    /// `ReloadTier::Reassemble`.
    Patched,
    /// `ReloadTier::Restart`.
    Restart,
}

pub trait DevReloadHook: 'static {
    // `attach`, `detach` and `poll` as in ADR-0311 §1.

    /// The entry's root view as the code now loaded builds it, taken once after a poll that
    /// reported `Patched` or `Restart`. `None` keeps the root the host was started with.
    fn root(&mut self) -> Option<BoxedView> {
        None
    }
}
```

- The host maps `Patched` to `Reassemble` and `Restart` to `Restart`. `ProcessRestart` has no
  event: the supervisor that started the process replaces it, and only after the new build
  succeeds, so a build error leaves the old window on screen.
- The loop records one pending tier beside the epoch of ADR-0311 §2. A UI runtime applies, at its
  frame boundary, the highest tier reported since it last applied one, once: a `Restart` that
  follows a `Patched` before the boundary is one restart, and a `Patched` that follows a `Restart`
  does not lower it.
- `root` is user code and is lent and contained as ADR-0311 §5 contains `poll`.

§4. **`Restart` remounts the root in the same UI runtime.** `Presentation::apply_hot_reload` with
`ReloadTier::Restart`:

1. unmounts the root, so every `State` is disposed in tree order by the code that created it;
2. renews the scene's lineage (ADR-0304 §7), so no recording of the old tree is replayed;
3. mounts the root from `DevReloadHook::root`, or a clone of the root the host was started with.

It keeps what the framework made without application code: the window, the surface, the GPU
context and its pipelines, and the text context with its font collection. A cache of the UI
runtime whose keys and values are framework types is kept; one that holds an application's type is
cleared. A window that application code opened is closed with the `State` that owned it. A panic
in a dispose or in the new root's mount is contained as a build's panic is (ADR-0287), and the
next reload applies as usual.

§5. **The tool says what it did.** For every reload `flui run` reports the tier, the reason (the
first changed file and, for a `Restart`, the item whose shape changed) and the time from the save
to the frame. When the mechanism leaves code running that the edit replaced, the report names it
and offers the restart (ADR-0394 §4).

§6. **The tiers hold on every host that drives `DevReloadHook`.** Where no mechanism loads new code
(the web, and widget code on Android), the tool applies `ProcessRestart`, or the page reload that
stands for it, and says so.

### Consequences

- Good, because an edit to a type costs a remount in the same window instead of a new process.
- Good, because the author marks nothing, and soundness does not rest on the author's discipline.
- Good, because one party decides: the tier is derived from the edit, not from flags an
  application and a tool would both have to keep.
- Bad, because the tool parses the changed sources on every save.
- Bad, because the shape is conservative: an edit to an attribute or to a `const` initialiser
  restarts where a finer rule could reassemble.
- Bad, because a statement macro inside a body that declares a type is outside the shape; the
  mechanism's record says how far that case is guarded (ADR-0394 §3).
- Bad, because `Restart` has no producer until a mechanism can load a whole application's code
  again: the worker path links `State` types into the host and cannot.
- Neutral, because `ReloadTier` keeps its three variants.

### Confirmation

- **The compiler.** `ReloadEvent` stays exhaustive, so a host that does not map `Restart` does not
  compile.
- **Tests the implementing change adds**, each through `flui_testing::host::HeadlessHost` or the
  owning crate's suite:
  `a_restart_remounts_the_root_with_fresh_state_and_keeps_the_window`,
  `a_restart_disposes_every_state_before_the_new_root_mounts`,
  `a_restart_after_a_patch_in_one_boundary_applies_once_as_a_restart`,
  `a_patch_after_a_restart_does_not_lower_the_tier`,
  `a_panicking_root_is_contained_and_the_next_reload_applies`; in `flui-app`,
  `each_reload_event_maps_to_the_ui_runtime_tier` gains the `Restart` row; in `flui-cli`, the
  table test `an_edit_is_placed_by_its_shape` with a row for a body edit, a field added, a field's
  type changed, a doc comment, a `const fn` body, an item declared inside a body, an item macro,
  a manifest and a source that does not parse.
- [`docs/gaps.md`](../gaps.md) lists this record under "Accepted ADR sections not built yet" until
  the sections are built.

## Pros and cons of the options

### 1. Two tiers

- Good, because nothing is built.
- Bad, because every edit to a type costs a process: about 2.9 s, a window that closes and opens,
  and shaders and fonts loaded again.
- Bad, because `ReloadTier::Restart` stays a variant that does something else.

### 2. Three tiers, chosen by the tool from the shape (chosen)

- Good, because it is Flutter's model with the same two fast outcomes.
- Good, because the decision needs nothing from the author and nothing from the running
  application.
- Bad, because the shape is a syntactic rule, so it restarts on some edits that could be kept.

### 3. Three tiers, declared by a derive on the author's types

- Good, because the hash is exact for the types that carry it.
- Bad, because every retained type needs the derive, which changes how widgets are written, and a
  type without it is unguarded.
- Bad, because the values a `State` reaches (a model behind an `Rc`, a closure's captures) are
  outside the derive.

### 4. Per-type tiers

- Good, because an edit to one `State` type would keep every other subtree.
- Bad, because a kept subtree may hold a value of the changed type behind a pointer, and nothing
  short of the shape of the whole crate shows that it does not.
- Neutral, because it can be added later as a refinement of `Restart` if a measurement asks for it.

## More information

- Flutter does not run `main` again on a hot reload and does on a hot restart
  (`run_hot.dart:604`); what a mechanism here runs again is its own decision (ADR-0394 §1).
- The `TypeId` run: a struct that gains a field and a closure that captures another type change
  their `TypeId`; a field whose type changes and an `async` block whose interior grows do not. It
  is why the tool, which sees the edit, decides, and why `TypeId` alone guards nothing.
- Revisit §2 when rustc can report whether a crate's interface changed apart from its bodies (the
  "relink, don't rebuild" goal), which would replace the syntactic shape with the compiler's own.
