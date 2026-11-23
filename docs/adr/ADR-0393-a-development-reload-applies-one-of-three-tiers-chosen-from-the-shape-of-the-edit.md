# ADR-0393: A development reload applies one of three tiers, chosen from the shape of the edit

- **Status:** Accepted (2026-11-23)
- **Date:** 2026-11-23
- **Supersedes:** [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md)
  §1, §2 and §7 only, in part: `ReloadEvent`'s two variants, the rule that a reported reload is
  always a reassemble, and the sentence that a restart event arrives with a code patcher's record
- **Related:** [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md) (the
  hook that reports a reload);
  [ADR-0394](ADR-0394-application-code-reloads-as-an-image-the-element-tree-adopts.md) (a proposed
  mechanism that loads new code); [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) (the
  rejected patcher, whose structural hash is option 3 here);
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the runtime names no tool);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) and
  [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (containment
  of the user code a restart runs);
  [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) §10 (a
  retired scene owner and the lineage of the one that follows it);
  [ADR-0326](ADR-0326-one-entry-starts-an-application-on-every-platform.md) (the entry a restart runs again)
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
| A hook reports `Unchanged` or `Patched`; the loop keeps one epoch and one tier for all its UI runtimes, and each applies a reload it has not seen at its own frame boundary | `ReloadEvent` in `crates/flui-view/src/dev_reload.rs`; `Slot` and `poll_and_apply` in `crates/flui-app/src/app/hot_reload.rs` |
| What an application's entry hands the loop is more than a root: its capability providers, keyed by `TypeId`, its plugins and its handlers, built once and handed to every UI runtime | `Application` in `crates/flui-app/src/app/launch.rs`; `CapabilityRegistry` in `crates/flui-runtime/src/capability.rs` |
| A UI runtime owns its text context and font collection, and its id is a generational key | `crates/flui-runtime/src/ui_runtime/construct.rs`; `UiRuntimeId` in `crates/flui-foundation/src/id.rs` |
| An edit that cannot keep `State` replaces the process: every edit in a project from `flui create`, and an edit to the types crate of one from `flui create --hot-reload`. The tool stops the process and then builds | [`docs/hot-reload.md`](../hot-reload.md); `on_change` of the process strategy in `crates/flui-cli/src/commands/run.rs` |
| A process restart after a one-line edit takes about 2.9 s, of which about 0.25 s is the start to the first frame; a worker reload takes 1.5–2.0 s | run of 2026-11-23, generated counter, Linux x86_64, 4 cores |
| Between two builds of one crate with the same `-C metadata`, a type's `TypeId` is unchanged when a field's type changes and when an `async` block keeps more across an `.await`, while its size changes | run of 2026-11-23, rustc 1.100.0 |
| Flutter keeps `State` across a hot reload and rebuilds every element without running `main`; a hot restart runs `main` again in a new isolate, in the same engine and window | Sources |

Rust has no VM that could give a live object a new layout, so an edit that changes a type cannot
keep the values of that type. Which edits keep `State`, what happens to the others, who decides,
and who owns the record of what each tree was built from?

## Decision drivers

- Flutter's loop is the model: an edit shows in under a second with `State` kept, and when it
  cannot be kept the tool says so and starts the application again without losing the window.
- An author writes widgets as before: no derive, attribute or registration exists for reload.
- New code never reads a value an older build laid out differently. A slower tier is the price of
  an edit the tool cannot place; a crash in the middle of an edit is not.
- A window, its surface, the GPU context with its pipelines, and the font collection do not depend
  on application code, and recreating them is what a process restart spends its start on.
- One owner for each piece of reload state: which build a tree was made from is recorded once, and
  a tier is derived from it, not stored beside it.
- The hook names no tool and the runtime names no host (ADR-0311, ADR-0083).
- CI executes Linux only, so each rule is held by a headless test.
- The author learns which tier ran and why, from what the process did.

## Considered options

1. Two tiers, as built: reassemble, and replace the process for everything else.
2. Three tiers chosen by the tool from the shape of the edit, where a restart remounts the root
   inside each UI runtime.
3. Three tiers chosen by the tool from the shape of the edit, where a restart retires the UI
   runtimes and runs the application's entry again.
4. Three tiers, and the author's types declare what can be kept: a derive writes a structural hash
   for every retained `View` and `ViewState` (ADR-0094 §3).
5. Per-type tiers: remount only the subtrees whose `State` type changed and keep the rest.

## Decision

Chosen option: 3, because it gives the author Flutter's two fast outcomes without asking them to
mark anything, because the one party that sees the whole edit decides, and because a restart then
has one rule (what the entry made is made again) instead of one rule per kind of state the entry
left behind.

§1. **A reload applies one of three tiers.**

| Tier | What it does | What survives |
|---|---|---|
| Reassemble | a UI runtime rebuilds every element with the code now loaded | every `State` whose view type is unchanged, with its signals, focus, scroll offset and routes |
| Restart | the loop retires every UI runtime and composes the application again from the entry of the code now loaded | the process, the event loop, the windows the runner opened with their size, position and state, their surfaces, the GPU context and its pipelines, each window's text context and font collection |
| Process restart | the supervisor replaces the process | nothing |

Flutter's counterparts are hot reload, hot restart and a full restart, and as there the entry
does not run on a reassemble and runs again on a restart.

§2. **The tool chooses the tier, from the shape of the edit, and stamps the build with it.** The
*shape* of a crate is its source with function bodies taken out. `flui run` computes it from the
tokens of every `.rs` file of the crate, as `syn` parses them:

- In the shape: every item with its attributes; signatures, generics and `where` clauses; fields
  and variants; trait and `impl` headers; `const` and `static` initialisers; the body of a
  `const fn`, since a type's layout may be computed from it; every token of a macro invocation in
  item position; and each item declared inside a body, wherever it is nested.
- Outside it: the block of a function, method or closure apart from the items it declares;
  comments and doc attributes; spans and whitespace.

A *reloadable crate* is one whose code the installed mechanism loads again in the running
process. The mechanism tells the tool which crates those are.

| The edit, since the build the application runs | Tier |
|---|---|
| The shape of every reloadable crate is unchanged | Reassemble |
| The shape of a reloadable crate changed; a file of one that the build read and the tool does not parse changed; the author asked for a restart | Restart |
| Anything else the build's dependency information names (a manifest, the lock file, a build script, a crate that is not reloadable); the author asked for a process restart | Process restart |
| The sources do not build | none: the running application is left as it is and the error is shown |

- The tool is the one decider. It errs upward: what it cannot place takes the higher tier. The
  author may ask for a higher tier than the tool chose, as Flutter's `R` does, and never a lower
  one.
- Each build the tool hands a mechanism carries a `CodeVersion` (§3). The tool gives a build a new
  `layout` exactly when it places the edit at Restart, and the layout of the build before it
  otherwise. Nothing downstream compares shapes or derives a tier a second time.
- A mechanism that cannot restart in the process takes a process restart where the table says
  Restart, and the tool applies it. The worker path is one: its `State` types are linked into the
  host. An edit that changes the shape of its worker crate therefore replaces the process, where
  today it reloads with nothing guarding the element that an older worker built.
- The supervisor replaces the process only after the new build succeeds, so a build error leaves
  the old window on screen at every tier.

§3. **The hook reports the version loaded, and the loop derives the tier from its own record.**
In `flui_view::dev_reload`:

```rust
/// The build a tree is made from. The hook mints it from what the tool stamped; the loop
/// compares it for equality and never orders or interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeVersion {
    /// Equal for two builds whose kept values the later build may read.
    pub layout: LayoutEpoch,
    /// Differs for every build.
    pub revision: Revision,
}

/// Exhaustive: a host derives the tier from the version and its own record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadEvent {
    Unchanged,
    /// New code is loaded and this is its version.
    Loaded(CodeVersion),
}

pub trait DevReloadHook: 'static {
    // `attach`, `detach` and `poll` as in ADR-0311 §1.

    /// Restart: run the entry of the code now loaded, on the owner thread. The entry hands its
    /// application to the loop that called this, as it handed the first one. The default runs
    /// nothing, for a mechanism that cannot restart in the process.
    fn run_entry(&mut self) {}

    /// What the loop did with a version, after the frame that applied it.
    fn applied(&mut self, outcome: &ReloadOutcome<'_>) {}
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReloadOutcome<'a> {
    /// One UI runtime reassembled. `behind` names the view types of the elements that still run
    /// code of an earlier build.
    Reassembled { version: CodeVersion, runtime: UiRuntimeId, behind: &'a [&'static str] },
    /// The loop restarted: because the layout changed, or because a reassemble was `raised`.
    Restarted { version: CodeVersion, raised: Option<RaiseReason> },
    /// The restart did not happen and the application that was running still is.
    RestartRefused { version: CodeVersion, reason: RestartRefusal },
}

/// Opaque: two are equal or they are not. The tool mints them and the hook carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutEpoch(NonZeroU64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revision(NonZeroU64);

#[derive(Debug)]
#[non_exhaustive]
pub enum RaiseReason {
    /// A mechanism handed back the element of this view type instead of reassembling it.
    ElementRefused { view: &'static str },
}

#[derive(Debug)]
#[non_exhaustive]
pub enum RestartRefusal {
    EntryPanicked,
    /// The entry returned without handing an application over.
    NoApplication,
    /// The entry's application is one `run` refuses; the error is the host's.
    Launch(Box<dyn std::error::Error>),
}
```

- **One record, in the loop.** Beside each event loop's hook instance the host keeps the version
  its UI runtimes were composed from (`composed`), the last version the hook reported (`loaded`)
  and, per UI runtime, the revision it last reassembled to. `composed` is written when the
  application is first composed and when a restart succeeds, and nowhere else. No tier is stored.
- **The tier is derived.** When `loaded.layout` differs from `composed.layout`, the loop restarts
  (§4) at its next turn, before any UI runtime's frame. Otherwise each UI runtime whose revision
  differs from `loaded.revision` reassembles at its own frame boundary, as ADR-0311 §2 has it
  apply a reload, and records the revision. A version the hook reports twice changes nothing.
- **A mechanism may raise and never lowers.** A hook that cannot place a build mints a layout no
  earlier version had. A UI runtime that cannot finish a reassemble (its mechanism refuses an
  element) raises: the loop restarts to the same `loaded` version at its next turn and the outcome
  carries the reason. No path turns a changed layout into a reassemble.
- **Process restart has no event.** The supervisor that started the process replaces it. A hook
  that needs one (an image it cannot load) says so to its tool on its own channel and reports
  `Unchanged`.
- **The outcome is the report's source.** The host calls `applied` once per UI runtime that
  reassembled and once per restart or refusal. A panic in `poll` or `applied` retires the hook as
  ADR-0311 §5 says. A panic in `run_entry` is the application's entry panicking: it refuses that
  restart and the hook stays.

§4. **A restart retires the UI runtimes and composes the application again.** The host:

1. runs `DevReloadHook::run_entry` and takes the application the entry hands over: its
   `Application` and its root. An entry that panics, returns an error or returns without an
   application refuses the restart, and so does an application `run` would refuse
   (`LaunchError`). A refused restart changes nothing: the UI runtimes keep running, `composed`
   keeps its value, the outcome is `RestartRefused`, and the next version the hook reports is
   compared with `composed` as any other, so a later build restarts again.
2. retires every UI runtime of the loop: each unmounts its root, so every `State` is disposed in
   tree order by the code that created it. A dispose that panics does not stop the unmount; the
   first payload is kept (`FirstPanic`) and logged. Each presentation detaches from its surface,
   and each window the application opened is closed.
3. composes from the application of step 1 what `run` composes at the start: the capability
   registry, the plugins and the handlers of the later `Application`, and for each window the
   runner opened a new UI runtime, which takes over that window's text context, mounts the root
   and attaches to the window's surface. A new scene owner has a new lineage, so its first
   publication is drawn in full (ADR-0304 §10).
4. writes `composed` and reports `Restarted`.

- A retired `UiRuntimeId` is never issued again, so work the earlier application started and that
  finishes later (a completion, a queued command, an answer addressed to a presentation) is
  refused by its id, as it is for a closed window. Nothing of the earlier application is kept by
  key and value type.
- A panic in the new root's mount shows what a build's panic shows (ADR-0287). `composed` is
  written all the same: the runtimes that exist were made from that version.
- The later application's window description is not applied to a kept window. An author who
  changed it asks for a process restart.
- The loop owns the list of its windows and which of them the runner opened. A window the
  application opened belongs to the application that opened it and goes with it.

`flui_runtime::reload::ReloadTier` is deleted with its `Restart` and `ProcessRestart` variants:
a UI runtime reassembles, and the two higher tiers are not operations on a UI runtime.

§5. **The tool reports what the process did.** For every build `flui run` prints the tier, the
reason (the first changed file and, for a restart, the item whose shape changed), the time from
the save to the outcome, and what the outcome names: a raise with its reason, a refusal with its
reason, the view types left behind. A build with no outcome within the tool's deadline is reported
as unanswered. When a mechanism knows of code an edit replaced that keeps running, its record says
how the report names it.

§6. **The tiers hold on every host that drives `DevReloadHook`.** Where no mechanism loads new
code, the tool applies a process restart, or the page reload that stands for it on the web, and
says so. Which hosts have a mechanism is the mechanism's record and
`crates/flui-native/platform-parity.toml`.

### Consequences

- Good, because an edit to a type costs a new composition in the same window instead of a new
  process.
- Good, because the author marks nothing, and soundness does not rest on the author's discipline.
- Good, because a restart has one rule. The capability registry, the plugins, the handlers, the
  caches of a UI runtime, the queues and the windows the application opened need no rule each.
- Good, because the entry runs when Flutter's `main` runs.
- Good, because no tier is stored: a window that draws late, two saves that arrive as one, and a
  restart that failed are all answered by comparing versions.
- Bad, because the tool parses the changed sources on every save.
- Bad, because the shape is conservative: an edit to an attribute or to a `const` initialiser
  restarts where a finer rule could reassemble.
- Bad, because a statement macro inside a body that declares a type is outside the shape, so a
  mechanism needs its own guard for a value whose size changed.
- Bad, because a restart builds a UI runtime again: its element, render and semantics trees and
  its caches start empty.
- Bad, because the worker path restarts the process on an edit that today reloads.
- Bad, because nothing restarts in the process until a mechanism implements `run_entry`.

### Confirmation

Each decision and what holds it. The tests are added by the changes that build the sections; the
loop's record lives in `flui-app`, so its tests run there on the headless platform with a
scripted hook from the testing kit.

| Decision | Held by |
|---|---|
| §2 placement by shape | `flui-cli` table test `an_edit_is_placed_by_its_shape`: a body edit, a field added, a field's type changed, a doc comment, a `const fn` body, an item declared inside a body, an item macro, a file the build read and the tool does not parse, a manifest, the lock file, a build script, a crate that is not reloadable, the author's two requests, and sources that do not build |
| §2 a new layout exactly at Restart | `flui-cli`: `a_restart_tier_build_gets_a_new_layout_and_a_reassemble_keeps_it` |
| §2 the worker path restarts the process on a changed worker shape | `flui-cli`: `a_changed_worker_shape_restarts_the_process` |
| §2 the old process runs until the build succeeds | `flui-cli`: `a_failed_build_leaves_the_running_process` |
| §3 the tier is derived per UI runtime | `flui-app`: `a_window_that_draws_late_reassembles_to_the_latest_revision`, `a_changed_layout_restarts_and_a_later_body_edit_reassembles`, `two_versions_before_a_boundary_apply_once`, `a_version_reported_twice_changes_nothing` |
| §3 a raise restarts to the same version | `flui-app`: `a_raised_reassemble_restarts_the_loop` |
| §3 the outcome | `flui-app`: `each_reassembled_runtime_and_each_restart_is_reported_once` |
| §3 `ReloadEvent` is exhaustive | the compiler: a host that does not map `Loaded` does not compile |
| §4 step 1 | `flui-app`: `a_panicking_entry_refuses_the_restart_and_the_next_version_restarts`, `an_entry_without_an_application_refuses_the_restart`, `a_refused_restart_keeps_every_state` |
| §4 step 2 | `flui-app`: `a_restart_disposes_every_state_before_the_new_root_mounts`, `a_panicking_dispose_does_not_stop_a_restart`, `a_restart_closes_the_windows_the_application_opened` |
| §4 step 3 | `flui-app`: `a_restart_composes_the_later_applications_capabilities`, `a_restart_keeps_the_window_and_its_text_context`, `an_answer_to_a_retired_runtime_is_refused` |
| §4 `ReloadTier` is gone | the compiler |
| §5 the report | `flui-cli`: `the_report_names_the_tier_the_reason_and_the_outcome`, `a_build_without_an_outcome_is_reported_unanswered` |
| §6 no mechanism | `flui-cli`: `a_target_without_a_mechanism_restarts_the_process` |

- What a restart keeps on a real surface (the window does not close, the pipelines are not built
  again) cannot be seen on the headless platform: the change that builds §4 adds its row to
  `crates/flui-native/platform-parity.toml`.
- [`docs/gaps.md`](../gaps.md) lists this record under "Accepted ADR sections not built yet" until
  the sections are built.

## Pros and cons of the options

### 1. Two tiers

- Good, because nothing is built.
- Bad, because every edit to a type costs a process: about 2.9 s, a window that closes and opens,
  and shaders and fonts loaded again.
- Bad, because `ReloadTier::Restart` stays a variant that does something else.

### 2. Three tiers, a restart remounts the root inside each UI runtime

- Good, because the UI runtime's trees are the only thing rebuilt.
- Bad, because the entry's other products stay from the first build: a capability provider keyed
  by `TypeId` answers with the old layout of a handle the new code reads.
- Bad, because each kind of kept state then needs its own rule (which caches to clear, which
  queued work to refuse, who closes a window the application opened), and each rule is a place
  for the next order of events to be missed.
- Bad, because every UI runtime needs a root of the new code, and one root handed over once does
  not serve two.

### 3. Three tiers, a restart runs the entry again (chosen)

- Good, because it is Flutter's hot restart: a new isolate there, new UI runtimes here, the same
  engine and window in both.
- Good, because what survives is what the host made before any application code ran.
- Good, because stale work is refused by an id the framework already mints.
- Bad, because a UI runtime is built again, which costs more than a remount.

### 4. Three tiers, declared by a derive on the author's types

- Good, because the hash is exact for the types that carry it.
- Bad, because every retained type needs the derive, which changes how widgets are written, and a
  type without it is unguarded.
- Bad, because the values a `State` reaches (a model behind an `Rc`, a closure's captures) are
  outside the derive.

### 5. Per-type tiers

- Good, because an edit to one `State` type would keep every other subtree.
- Bad, because a kept subtree may hold a value of the changed type behind a pointer, and nothing
  short of the shape of the whole crate shows that it does not.
- Neutral, because it can be added later as a refinement of Restart if a measurement asks for it.

## More information

- The `TypeId` run: a struct that gains a field and a closure that captures another type change
  their `TypeId`; a field whose type changes and an `async` block whose interior grows do not. It
  is why the tool, which sees the edit, decides, and why `TypeId` alone guards nothing.
- Revisit §2 when rustc can report whether a crate's interface changed apart from its bodies (the
  "relink, don't rebuild" goal), which would replace the syntactic shape with the compiler's own.
- Revisit §4's cost when a restart is measured on a real application: the element and render
  trees are rebuilt in both forms of restart, so the difference is the UI runtime's own
  construction.
