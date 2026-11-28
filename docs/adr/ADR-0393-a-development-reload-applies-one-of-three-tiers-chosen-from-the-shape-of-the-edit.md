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
  rejected patcher, whose structural hash is option 4 here);
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the runtime names no tool);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) and
  [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (containment
  of the user code a restart runs);
  [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) §10 (a
  retired scene owner and the lineage of the one that follows it);
  [ADR-0326](ADR-0326-one-entry-starts-an-application-on-every-platform.md) (the entry a restart runs again)
- **Sources:** `flutter/flutter@3.50.1` (`ae9b586f34`)
  `packages/flutter/lib/src/foundation/binding.dart:742` (`reassembleApplication`),
  `packages/flutter/lib/src/widgets/binding.dart:1715` (`performReassemble`),
  `packages/flutter/lib/src/widgets/framework.dart:1053` (`State.reassemble`), `:3471`
  (`BuildOwner.reassemble`), `:3772` (`Element.reassemble`);
  `packages/flutter_tools/lib/src/run_hot.dart:604` (`_restartFromSources`, which kills the
  isolates the UI isolate spawned before `main` runs again), `:759` (a rejected reload), `:1586`
  (the advice to restart)

## Context and problem statement

An application author edits code and waits for the running application to show it. What `main`
holds:

| Fact | Evidence |
|---|---|
| The runtime names three tiers: `Reassemble`, `Restart`, `ProcessRestart` | `ReloadTier` in `crates/flui-runtime/src/reload.rs` |
| Only `Reassemble` is built: `Restart` logs that the root remount is not implemented and reassembles | `Presentation::apply_hot_reload` in `crates/flui-runtime/src/presentation.rs` |
| A hook reports `Unchanged` or `Patched`; the loop keeps one epoch and one tier for all its UI runtimes, and each applies a reload it has not seen at its own frame boundary | `ReloadEvent` in `crates/flui-view/src/dev_reload.rs`; `Slot` and `poll_and_apply` in `crates/flui-app/src/app/hot_reload.rs` |
| What an application's entry hands the loop is more than a root: capability providers keyed by `TypeId`, plugins, handlers, services, an exit policy and a main-window factory, from which the loop builds state of its own once per run | `Application` in `crates/flui-app/src/app/launch.rs`; `crates/flui-app/src/app/runtime.rs`; `crates/flui-app/src/app/runner/main_window.rs` |
| `Application::run` starts and drives the platform loop and returns when the run ends; only Android hands something into a run already going | ADR-0326; `hand_android_entry` in `crates/flui-app/src/app/runner/mod.rs` |
| A UI runtime owns its text context and font collection, and its id is a generational key | `crates/flui-runtime/src/ui_runtime/construct.rs`; `UiRuntimeId` in `crates/flui-foundation/src/id.rs` |
| An edit that cannot keep `State` replaces the process: every edit in a project from `flui create`, and an edit to the types crate of one from `flui create --hot-reload`. The tool builds beside the running process and replaces it once the build succeeds; on Windows, which refuses to overwrite a running executable, it stops the process first | [`docs/hot-reload.md`](../hot-reload.md); `on_change` of the process strategy in `crates/flui-cli/src/commands/run/process_restart.rs` |
| A process restart after a one-line edit takes about 2.9 s, of which about 0.25 s is the start to the first frame and the rest is the build; a worker reload takes 1.5–2.0 s | run of 2026-11-23, generated counter, Linux x86_64, 4 cores |
| Between two builds of one crate with the same `-C metadata`, a type's `TypeId` is unchanged when a field's type changes and when an `async` block keeps more across an `.await`, while its size changes | run of 2026-11-23, rustc 1.100.0 |
| Flutter keeps `State` across a hot reload and rebuilds every element without running `main`; a hot restart ends the isolates of the running program and runs `main` again in a new isolate, in the same engine and window | Sources |

Rust has no VM that could give a live object a new layout, so an edit that changes a type cannot
keep the values of that type. Which edits keep `State`, what happens to the others, who decides,
and who owns the record of what each tree was built from?

## Decision drivers

- Flutter's loop is the model: an edit shows in under a second with `State` kept, and when it
  cannot be kept the tool says so and starts the application again without losing the window.
- An author writes widgets as before: no derive, attribute or registration exists for reload.
- New code never reads a value an older build laid out differently. A slower tier is the price of
  an edit the tool cannot place; a crash in the middle of an edit is not.
- A window, its surface and the GPU context with its pipelines do not depend on application code,
  and keeping them keeps the window on screen, where a process restart closes it and builds them
  again.
- One owner for each piece of reload state: which build a tree was made from is recorded once, and
  a tier is derived from it, not stored beside it.
- The hook names no tool and the runtime names no host (ADR-0311, ADR-0083).
- CI executes Linux only, so each rule is held by a headless test.
- The author learns which tier ran and why, from what the process did.

## Considered options

1. Two tiers, as built: reassemble, and replace the process for everything else.
2. Three tiers chosen by the tool from the shape of the edit, where a restart remounts the root
   inside each UI runtime.
3. Three tiers chosen by the tool from the shape of the edit, where a restart retires the
   application and runs its entry again.
4. Three tiers, and the author's types declare what can be kept: a derive writes a structural hash
   for every retained `View` and `ViewState` (ADR-0094 §3).
5. Per-type tiers: remount only the subtrees whose `State` type changed and keep the rest.

## Decision

Chosen option: 3, because it gives the author Flutter's two fast outcomes without asking them to
mark anything, because the one party that sees the whole edit decides, and because a restart then
has one rule (what the application made is dropped and made again) instead of one rule per kind
of state the application left behind.

§1. **A reload applies one of three tiers.**

| Tier | What it does | What survives |
|---|---|---|
| Reassemble | a UI runtime rebuilds every element with the code now loaded | every `State` whose view type is unchanged, with its signals, focus, scroll offset and routes |
| Restart | the loop retires the application and composes it again from the entry of the code now loaded | the process, the event loop, the windows the runner opened with their size, position and state, their surfaces, the GPU context and its pipelines, and the fonts the system provides as they were loaded |
| Process restart | the supervisor replaces the process | nothing |

Flutter's counterparts are hot reload, hot restart and a full restart, and as there the entry
does not run on a reassemble and runs again on a restart.

§2. **The tool chooses the tier, from the shape of the edit, and stamps the build with it.** The
*shape* of a crate is its source with function bodies taken out. `flui run` computes it from the
tokens of every `.rs` file of the crate, as `syn` parses them:

- In the shape: every item with its attributes; signatures, generics and `where` clauses; fields
  and variants; trait and `impl` headers; `const` and `static` initialisers; the body of a
  `const fn`, since a type's layout may be computed from it; the body of an `async fn` and of an
  `async` block, since what it keeps across an `.await` is the layout of its future; every token
  of a macro invocation in item position, and of one inside a body whose tokens hold an item
  keyword (`struct`, `enum`, `union`, `type`, `impl`, `fn`, `static`); and each item declared
  inside a body, wherever it is nested.
- Outside it: the rest of the block of a function, method or closure; comments and doc
  attributes; spans and whitespace.

A *reloadable crate* is one whose code the installed mechanism loads again in the running
process. The mechanism tells the tool which crates those are.

| The edit, since the build the tool staged before | Tier |
|---|---|
| The shape of every reloadable crate is unchanged | Reassemble |
| The shape of a reloadable crate changed; a file of one that the build read and the tool does not parse changed; the author asked for a restart | Restart |
| Anything else the build's dependency information names (a manifest, the lock file, a build script, a crate that is not reloadable); the author asked for a process restart | Process restart |
| The sources do not build | none: the running application is left as it is and the error is shown |

- The tool is the one placer. It errs upward: what it cannot place takes the higher tier. The
  author may ask for a higher tier than the tool chose, as Flutter's `R` does, and never a lower
  one. The loop applies the tier each version implies (§3), and two builds staged before one frame
  boundary are applied as one at the higher of their tiers.
- The tool mints every version: a `CodeVersion` (§3) per staged build, from one `IdMint` per
  `flui run` session, written into the build's manifest as integers. A build gets a new `layout`
  exactly when the tool places it at Restart, and the layout of the build staged before it
  otherwise. Nothing downstream mints a version or compares shapes.
- A manifest belongs to one process. A build placed at Process restart is staged only for the
  process that replaces the running one, so the running process never loads a build made against
  other dependencies.
- A mechanism that cannot restart in the process takes a process restart where the table says
  Restart, and the tool applies it. The worker path is one: its `State` types are linked into the
  host. An edit that changes the shape of its worker crate therefore replaces the process, where
  today it reloads with nothing guarding the element that an older worker built.
- The supervisor replaces the process only after the new build succeeds, so a build error leaves
  the old window on screen at every tier. A process runs from copies of its executable and
  libraries staged under content-addressed names, so a build never writes a file the running
  process has mapped, which Windows refuses.

§3. **The hook reports the version loaded, and the loop derives the tier from its own record.**
In `flui_view::dev_reload`:

```rust
/// The build a tree is made from. The tool mints it and the hook reads it from the build's
/// manifest; the loop compares it for equality and never orders or interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeVersion {
    /// Equal for two builds whose kept values the later build may read.
    pub layout: LayoutEpoch,
    /// Differs for every build.
    pub revision: Revision,
}

/// Opaque: two are equal or they are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutEpoch(NonZeroU64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revision(NonZeroU64);

impl LayoutEpoch {
    /// The epoch the tool wrote into a manifest; `None` for zero.
    pub fn try_new(raw: u64) -> Option<Self>;
}
impl Revision {
    pub fn try_new(raw: u64) -> Option<Self>;
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

    /// Restart: run the entry of the code now loaded, on the owner thread, while the loop's
    /// hand-over is open (§4 step 2). The default runs nothing, for a mechanism that cannot
    /// restart in the process.
    fn run_entry(&mut self) {}

    /// What the loop did with a version, after the turn that applied it.
    fn applied(&mut self, outcome: &ReloadOutcome<'_>) {}

    /// At the same boundary, after `poll`: whether the code already loaded asked every UI
    /// runtime to rebuild (a worker's own request). The loop reassembles each UI runtime once,
    /// outside the record, and reports no outcome; as Flutter's `ext.flutter.reassemble`, it
    /// delivers no code.
    fn reassemble_requested(&mut self) -> bool { false }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReloadOutcome<'a> {
    /// One UI runtime reassembled to `version`.
    #[non_exhaustive]
    Reassembled { version: CodeVersion, runtime: UiRuntimeId },
    /// The application was composed again from `version`; `unapplied` is what the later
    /// application configured that the restart left as the process started it (§4).
    #[non_exhaustive]
    Restarted { version: CodeVersion, unapplied: Unapplied },
    /// The restart did not compose an application; the loop has none until the next one does.
    #[non_exhaustive]
    RestartFailed { version: CodeVersion, reason: RestartFailure<'a> },
}

/// What a restart did not apply; a process restart applies each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unapplied {
    /// The later application configures its executors; the loop keeps its pools.
    pub executors: bool,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum RestartFailure<'a> {
    EntryPanicked,
    /// The entry returned without handing an application over.
    NoApplication,
    /// The handed application is one `run` refuses, or composing it failed; the error is the
    /// host's.
    Launch(&'a dyn std::error::Error),
    /// User code that composing runs (a plugin's registration, a service's start) panicked.
    ComposePanicked,
}
```

- **One record, in the loop.** Beside each event loop's hook instance the host keeps the version
  its application was composed from (`composed`, empty after a restart that failed), the last
  version the hook reported (`loaded`) and, per UI runtime, the revision it was composed from or
  last reassembled to, removed when that UI runtime retires. `composed` is written when an
  application is composed, emptied when a restart fails, and written once more in a loop whose
  application was composed before the hook reported any version: the first version reported is
  taken as `composed`, and each UI runtime, whose revision is still unknown, reassembles to it
  once. Nothing else writes it. No tier is stored.
- **The tier is derived, at the poll that reports a version.** A version equal to `loaded`
  changes nothing. Otherwise the loop records it as `loaded`, and:
  - when `composed` is empty or its layout differs, the loop restarts to it (§4) when the frame in
    which it was polled returns, before the loop waits for the next event; no UI runtime
    reassembles in that frame;
  - when the layouts are equal, each UI runtime whose revision differs from `loaded.revision`
    reassembles at its own frame boundary, as ADR-0311 §2 has it apply a reload, and records the
    revision. A window that draws late reassembles to the latest version, once.
  A failed restart is not tried again for that version; the next version the hook reports is.
- **Where the loop polls.** At each UI runtime's frame boundary, as ADR-0311 §1 says, and, in a
  loop with no UI runtime, when the hook's wake arrives. The polling, the record and the restart
  are one module of `flui-app` that every host calls; no host writes them.
- **Process restart has no event.** The supervisor that started the process replaces it. A hook
  that needs one (an image it cannot load) says so to its tool on its own channel and reports
  `Unchanged`. A hook mints no version.
- **The outcome is the report's source.** The host calls `applied` once per UI runtime that
  reassembled and once per restart, succeeded or failed. A panic in `poll`, `reassemble_requested`
  or `applied` retires the hook as ADR-0311 §5 says. A panic in `run_entry` is the application's entry panicking: it fails
  that restart and the hook stays.
- **A mechanism may add ways for a reassemble to stop.** A mechanism whose record lets an element
  refuse new code, or leave code of an earlier build running, extends `Reassembled` and this
  record with them (ADR-0394 proposes both). Without one a reassemble always completes.

§4. **A restart retires the application and composes it again.** The loop holds what it builds
from an `Application` (the capability registry, plugins, handlers, services, the exit policy, the
main-window factory and its installer, the placement registry, the callback for opened files, the
tasks on the executors, the application-control ingress, the application's font registrations,
every UI runtime) in one value, the *application scope*, which a restart drops and builds again.
What the loop holds outside it is what the host made before any application code ran, and only
that is kept. The host, at the end of the turn:

1. **retires the application.** Each UI runtime unmounts its root, so every `State` is disposed in
   tree order by the code that created it, and its presentation detaches from its surface. A
   dispose that panics does not stop the unmount; the first payload is kept (`FirstPanic`) and
   logged. Each window the application opened is closed as the end of a run closes it: no close
   request is asked and no handler of the earlier application runs. Then the scope is dropped in
   the reverse of the order it was built in, its services cancelled and joined by the exit's
   deadline as at the end of a run, then the tasks and workers its services spawned, the first
   panic kept.
2. **runs the entry with the hand-over open.** The loop opens its hand-over, which belongs to this
   call: empty before it and drained after it on every exit. While it is open, `Application::run`
   on the owner thread hands its application and root to it and returns without starting a loop,
   and the entry's declarations (`#[flui::main]`'s `declare`) are recorded for the new scope. The
   loop holds no borrow of its own state across the call. An entry that panics, returns without
   handing an application over, or hands one `run` refuses (`LaunchError`) fails the restart. A
   second `run` in that entry is refused with `LaunchError::AlreadyHandedOver`, and the first
   application is the one composed.
3. **composes the handed application** as `run` composes it at the start: a new scope, and for
   each window the runner opened a new UI runtime, with a text context built from the kept system
   fonts and the application's registrations, that mounts the root and attaches to the window's
   surface. A new scene owner has a new lineage, so its first publication is drawn in full
   (ADR-0304 §10), and the window's accessibility tree is replaced in full. A UI runtime that
   cannot be built, a surface that cannot be attached, or composing code that panics fails the
   restart, and what step 3 composed is retired as in step 1. A panic in the new root's mount
   shows what a build's panic shows (ADR-0287) and does not fail it.
4. **writes `composed`**, answers the exit-policy question with the new application's policy, and
   reports `Restarted` once the install into the kept window has committed (or, with no window kept,
   once the main window it opens anew has). An install that commits
   in a later owner turn (one queued behind an owner callback) holds the outcome until it settles,
   and the next restart waits for it; one that fails as it commits fails the restart, and what step
   3 composed is retired as in step 1, with no window kept.

- A failed restart leaves the loop with the kept windows, their last frames and no application.
  `composed` is emptied and the outcome is `RestartFailed`. The tool, which receives the outcome,
  then applies a process restart, so the edit is shown either way; a later version the hook
  reports before that is tried.
- Work the earlier application addressed through the framework is refused after it is retired: a
  retired `UiRuntimeId` is never issued again, the earlier ingress refuses every request an
  `AppHandle` of it sends, and an accessibility action names a node of a tree that is gone. Every
  task and worker the application's services spawned is cancelled, whoever holds its handle, the
  running ones are waited for by the same deadline, one a pool still holds queued never starts,
  and a `TaskSpawner` of the earlier application refuses from then on. A thread the application
  spawned itself is its own: the framework cannot end it, and it keeps running code of the earlier
  build.
- The later application's window description is not applied to a kept window. An author who
  changed it asks for a process restart.
- The executor pools are the loop's, not the scope's: a host runs its own IO on them (ADR-0327
  §3), started before any application code. A later application's `AppConfig::executors` is not
  applied, and `ReloadOutcome::Restarted` names them in its `unapplied` (`Unapplied::executors`),
  which a process restart applies. Since the pools stay, a service that ignores its cancellation
  past the deadline is abandoned and its future keeps running, as does a compute job that never
  checks its cancellation, where the end of a run closes the pools under them.
- What the operating system opens is declared to it as the run starts (ADR-0307): the
  declarations, the single-instance choice and the registered associations stay the loop's until
  a process restart. The later application's `on_open_files` replaces the earlier one's, which
  drops with the scope.
- The development reload hook and the driver agent are the loop's, installed as it starts: a later
  application's `AppConfig::dev_reload` and agent settings are not installed.

`flui_runtime::reload::ReloadTier` is deleted, and the runtime's reload command loses its
argument: a UI runtime reassembles, and the two higher tiers are not operations on a UI runtime.
`HotReloadTier` of `packages/flui-hot-reload`, its second statement, is deleted in the same change.

§5. **The tool reports what the process did.** For every outcome `flui run` prints the tier the
outcome names, the tool's reason (the first changed file and, for a restart, the item whose shape
changed), the time from the save to the outcome, and the outcome's details: a failure with its
reason and the process restart that follows it. A build the next one replaced before the process
loaded it is reported as applied with the next. A build with no outcome within the tool's deadline
is handled as a failed restart: the tool applies a process restart and says so. When a mechanism
knows of code an edit replaced that keeps running, its record says how the report names it.

§6. **The tiers hold on every host that drives `DevReloadHook`.** Where no mechanism loads new
code, the tool applies a process restart, or the page reload that stands for it on the web, and
says so. Which hosts have a mechanism is the mechanism's record and
`crates/flui-native/platform-parity.toml`.

### Consequences

- Good, because an edit to a type costs a new composition in the same window instead of a new
  process.
- Good, because the author marks nothing, and soundness does not rest on the author's discipline.
- Good, because a restart has one rule: what is in the application scope is dropped and built
  again, and adding state to the scope needs no rule of its own.
- Good, because the entry runs when Flutter's `main` runs, after the earlier application is gone,
  as Flutter runs it after the earlier isolate is gone.
- Good, because no tier is stored: a window that draws late, two saves that arrive as one, and a
  restart that failed are all answered by comparing versions.
- Bad, because the tool parses the changed sources on every save.
- Bad, because the shape is conservative: an edit to an attribute, a `const` initialiser or any
  `async` body restarts where a finer rule could reassemble.
- Bad, because a macro invocation inside a body whose expansion declares a type without an item
  keyword among its tokens is outside the shape, so a mechanism needs its own guard for a value
  whose size changed.
- Bad, because a restart builds the UI runtimes again: their element, render and semantics trees
  and their caches start empty.
- Bad, because the worker path restarts the process on an edit that today reloads.
- Bad, because the entry runs again in the same process, where Flutter gives `main` a new isolate:
  an entry that sets a process-wide value a second time (a global log subscriber) fails every
  restart, and each of its Restart-tier edits costs a process restart.
- Bad, because a thread the earlier application spawned outlives a restart, where Flutter's
  restart ends the isolates the program spawned.
- Bad, because nothing restarts in the process until a mechanism implements `run_entry`.

### Confirmation

Each decision and what holds it. The tests are added by the changes that build the sections; the
loop's record lives in `flui-app`, so its tests run there on the headless platform with a
scripted hook from the testing kit. They replace ADR-0311's
`each_reload_event_maps_to_the_ui_runtime_tier`.

| Decision | Held by |
|---|---|
| §2 placement by shape | `flui-cli` table test `an_edit_is_placed_by_its_shape`: a body edit, a field added, a field's type changed, a doc comment, a `const fn` body, an `async fn` body, an `async` block in a body, an item declared inside a body, an item macro, a body macro with and without an item keyword, a file the build read and the tool does not parse, a manifest, the lock file, a build script, a crate that is not reloadable, the author's two requests, and sources that do not build |
| §2 versions | `flui-cli`: `a_restart_tier_build_gets_a_new_layout_and_a_reassemble_keeps_it`, `a_process_restart_build_is_staged_only_for_the_next_process` |
| §2 the worker path restarts the process on a changed worker shape | `flui-cli`: `a_changed_worker_shape_restarts_the_process` |
| §2 the old process runs until the build succeeds, from staged copies | `flui-cli`: `a_failed_build_leaves_the_running_process`, `a_process_runs_from_staged_copies`; the nightly Windows `flui-cli` scenarios run a process restart |
| §3 the tier is derived per UI runtime | `flui-app`: `a_window_that_draws_late_reassembles_to_the_latest_revision`, `a_changed_layout_restarts_and_a_later_body_edit_reassembles`, `two_versions_before_a_boundary_apply_once`, `a_version_reported_twice_changes_nothing`, `a_restart_tier_save_then_a_body_save_restart_once` |
| §3 where the loop polls | `flui-app`: `an_idle_loop_restarts_on_the_wake_that_reported_the_version`, `a_loop_without_a_ui_runtime_polls_on_the_wake` |
| §3 the outcome | `flui-app`: `each_reassembled_runtime_and_each_restart_is_reported_once`, `a_panicking_applied_retires_the_hook` |
| §3 `ReloadEvent` is exhaustive | the compiler: a host that does not map `Loaded` does not compile |
| §4 step 1 | `flui-app`: `a_restart_disposes_every_state_before_the_entry_runs`, `a_panicking_dispose_does_not_stop_a_restart`, `a_restart_closes_the_windows_the_application_opened_without_asking`, `a_restart_drops_the_earlier_scope`, `a_restart_cancels_and_joins_the_earlier_applications_tasks`, `an_app_handle_of_the_earlier_application_is_refused`; in `crates/flui-app/src/app/lifecycle.rs`, `shutdown_ends_the_applications_tasks_and_workers`, `a_queued_job_is_not_waited_for_and_never_starts`, `a_job_that_ignores_cancellation_is_reported_at_the_deadline` and `a_dropped_workers_state_goes_before_its_hold` |
| §4 step 2 | `flui-app`: `a_panicking_entry_fails_the_restart_and_is_not_tried_again`, `an_entry_without_an_application_fails_the_restart`, `a_failed_restart_leaves_nothing_in_the_hand_over`, `a_second_run_in_one_entry_is_refused_and_the_first_is_composed` |
| §4 step 3 | `flui-app`: `a_restart_composes_the_later_applications_capabilities`, `a_restart_keeps_the_window_and_registers_the_fonts_again`, `a_failed_composition_retires_what_it_built`, `a_panic_in_the_new_roots_mount_is_still_a_restart` |
| §4 step 4 | `flui-app`: `a_resident_application_is_asked_to_exit_by_its_new_policy`, `a_failed_install_into_the_kept_window_fails_the_restart`; `a_kept_install_that_commits_later_settles_when_it_does` in `crates/flui-app/src/app/runner/main_window.rs` |
| §4 what stays the loop's | `flui-app`: `opened_files_after_a_restart_reach_the_later_application`, `a_restart_reports_the_executors_it_did_not_apply`; a retired `UiRuntimeId` is not issued again because it is a generational key (ADR-0313) |
| §4 `ReloadTier` and `HotReloadTier` are gone | the compiler |
| §5 the report | `flui-cli`: `the_report_names_the_tier_the_reason_and_the_outcome`, `a_failed_restart_restarts_the_process`, `a_build_without_an_outcome_restarts_the_process`, `a_replaced_build_is_reported_with_the_next` |
| §6 no mechanism | `flui-cli`: `a_target_without_a_mechanism_restarts_the_process` |

What a restart keeps on a real surface (the window does not close, the pipelines are not built
again) cannot be seen on the headless platform: the change that builds §4 adds its row to
`crates/flui-native/platform-parity.toml`.

## Pros and cons of the options

### 1. Two tiers

- Good, because nothing is built.
- Bad, because every edit to a type closes the window and opens a new one, with the start to the
  first frame (about 0.25 s) and the shaders and fonts loaded again; the build is the same in
  every option.
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

### 3. Three tiers, a restart retires the application and runs the entry again (chosen)

- Good, because it is Flutter's hot restart: a new isolate there, a new application scope here,
  the same engine and window in both.
- Good, because what survives is what the host made before any application code ran, held apart
  by a type.
- Good, because stale work is refused by ids and an ingress the framework already has.
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

- Weighed inside option 3, the order of the two halves. Running the new entry before the earlier
  application is retired would keep that application on screen when the entry fails; but a
  failure is followed by a process restart, which discards it anyway, and the new entry would meet
  every resource the earlier one still holds (a port, a device, an audio context), failing or
  duplicating it. Retiring first is Flutter's order.
- The `TypeId` run: a struct that gains a field and a closure that captures another type change
  their `TypeId`; a field whose type changes and an `async` block whose interior grows do not. It
  is why the tool, which sees the edit, decides, why `async` bodies are in the shape, and why
  `TypeId` alone guards nothing.
- Revisit §2 when rustc can report whether a crate's interface changed apart from its bodies (the
  "relink, don't rebuild" goal), which would replace the syntactic shape with the compiler's own.
- Revisit §4's cost when a restart is measured on a real application: the element and render
  trees are rebuilt in both forms of restart, so the difference is the scope's own construction.
