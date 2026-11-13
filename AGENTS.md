# AGENTS.md

What the code doesn't tell you about FLUI, an open-source declarative UI framework for Rust:
`View` → `Element` → `RenderObject` → `Layer`, with `Semantics` alongside, rendered by
`flui-engine` through `wgpu`. Pre-1.0: fix a bad shape rather than work around it.

## Design stance

- **Not a Flutter port.** It keeps declarative widgets, the View/Element/RenderObject split and
  constraints-down layout; the rest is idiomatic Rust chosen on its merits, and code that
  differs from Flutter is usually deliberate (ADR-0027). Other frameworks (Flutter, Compose,
  SwiftUI, egui, Xilem, GPUI, …) are a checklist of edge cases, read with `gh` or a shallow
  clone into the gitignored `.flutter/` / `.gpui/`.
- **Rules are types:** the compiler first, a clippy lint next, then a tool CI already runs or a
  test of the owning crate; an `xtask` gate is the last rung, admitted by its ADR (ADR-0303). An
  exception is `#[expect(lint, reason = "…")]` at the site.
- **The frame path is synchronous:** no `async` in build/layout/paint, no lock on per-node state
  touched in `perform_layout`/`paint`, none in a public signature.
- **Layers, not micro-crates:** a feature inside a layer is a module; shared code moves down.

## Recurring defects

Each has shipped more than once:

- user code (callbacks, wakers, observers, `Drop`) re-entering with guards or borrows still held;
- cleanup after a caught panic that loses captures or the first failure (`thread::panicking()`
  is false after a catch);
- accepted work dropped when a hook is missing, replaced or panics, or an older wake clearing a
  newer obligation;
- identity compared by label instead of typed ID, stale generations, ID counters that wrap;
- overflow in intermediate arithmetic, non-finite geometry published;
- regression tests that pass with the fix reverted;
- workspace-wide changes that skip features, examples, shaders or tooling;
- a replacement that leaves the replaced form alive: a better primitive, API or rule lands, the
  old one keeps its callers and the documents that teach it, and the next change copies the old
  one (panic containment, frame demand, navigation and crate ordering each have two or three live
  generations on `main`).

## Current forms

Where a concept has more than one form on `main`, new code uses the form in the "Use" column.
The forms in the "Not" column are on their way out: copying one is a defect even where the
neighbouring code does it. A change that removes the last user of such a form deletes the form
and edits its row.

| Need | Use | Not |
|------|-----|-----|
| An owner changes its state, then runs code it does not own | `flui_foundation::panic::commit` / `commit_into` and `Outbox` (ADR-0287) | calling the listener, waker or `Drop` with the borrow or guard still held |
| Keep the first of several caught panics and resume it after cleanup | `FirstPanic`; a payload nobody resumes goes to `discard_payload` (ADR-0281) | a crate-local first-failure fold, `PanicRecovery` outside `flui-foundation`, retaining or `mem::forget`ting a payload |
| State only the owner thread touches | `Rc` with `Cell`/`RefCell`; a re-entry that finds it busy gets a typed outcome (ADR-0323 §1) | `Arc<Mutex<_>>`, atomics, `try_lock` as a re-entrancy detector, `Send + Sync` on a callback that never leaves the thread |
| A list of listeners | `Notifier<Arg>`, `ChangeNotifier`; a listener's owned handle is a `Subscription` from its channel's `listen` | a hand-rolled `Vec` of callbacks guarded by a `notifying: bool` |
| A hash table keyed by ids or fingerprints the process mints | `rustc_hash::FxHashMap`/`FxHashSet`; a per-frame set over tree ids is `flui_foundation::SlotSet`/`SlotIndex`; a crate holds it with `#![warn(clippy::disallowed_types)]` ([gaps](docs/gaps.md#hash-tables-outside-the-view-painting-and-engine-crates-still-use-siphash)) | std `HashMap`/`HashSet` with its SipHash seed; a table keyed by values an application or its content supplies (view-key hashes, authored sizes, font names and paths) keeps std with `#[expect(clippy::disallowed_types, reason = "…")]` |
| A new identity | `IdMint<I>` and `IdExhausted` (`flui_foundation::sequence`), its `AtomicIdMint` where two threads mint one id domain, or a generational key whose slot generations `GenSlots` carries (ADR-0313) | `wrapping_add`, a `pub` integer field, an ID derived from an OS handle or pointer |
| A state's signals and capability handles | created in `StatefulView::create_state(&self, ctx)`, so the state is born bound | an `Option` field that `init_state` fills with `Some(ctx.…)` ([gaps](docs/gaps.md#widget-states-still-fill-their-handles-in-init_state)) |
| An element holds the view it was built from | the shared `Rc<V>` that `View::create_element(self: Rc<Self>)` receives; a `BoxedView` hands its `Rc` on, so an element never copies its view (ADR-0245) | a clone of the view kept beside the parent's `BoxedView`, a `Weak` back to the source to compare identity |
| An application's test or a scenario that runs on every target | `flui_testing::Tester` (`flui::testing`) with `find` locators, acting through the window's agent | coordinates computed by hand, a `PointerScript` aimed at a node's bounds, root scopes (`MediaQuery`, `GestureArenaScope`, `FocusTreeRoot`) wrapped by the test |
| A widget navigates, opens a dialog or pops its page | `Router::<R>::of(ctx)` and `PageHandle::of(ctx)` resolved in `build`, the handle moved into the callback; a dialog or menu is a `PopupRoute` pushed with `PageHandle::push_popup` (ADR-0330) | a navigator handle: `Navigator`, `NavigatorHandle` and `WidgetsApp::navigator` left the public API (ADR-0330 §4), and inside `flui-widgets` only the navigator, the Router and the menus' popup placement reach `NavigatorHandle` |
| A test that drives an application's frames | `flui_testing::HeadlessHost` over `UiRuntime::pump` and its manual clock | `HeadlessBinding::pump_frame`, which ADR-0083 removes |
| Which crate may depend on which | `tier`, `order` and `reach` (ADR-0081) | reasoning from `layer = N`, which ADR-0081 removed |
| The platform whose conventions a widget follows (chords, handles, menus, transitions) | `flui_view::environment::PlatformConvention::of`, installed by the runtime above every root; a test names its platform with `HeadlessWindow::with_platform` or a `PlatformConvention` wrapper (ADR-0028) | `TargetPlatform::current()` outside the app runner, a theme's platform field, a `Default` that reads the build host |
| A platform's status, a version floor, an open gap | `crates/flui-native/platform-parity.toml` (rendered into `docs/platform-parity.md`), `platform-support.toml`, `docs/gaps.md` | a status line in an ADR, a spec or an `ARCHITECTURE.md`; a hand edit of `docs/platform-parity.md` |

## Codebase map

`docs/crates.md` maps the crates, `packages/` and the facade (`src/`); each manifest's
`[package.metadata.flui]` gives tier, kind and order. Not visible from the layout:

- only `flui-app` depends on `flui-native`, and `windows::*`/`objc2::*` types stay in it
  (ADR-0082); `flui-runtime` has no host, platform or GPU edge (ADR-0083);
- official packages build on `flui-sdk` alone (ADR-0088); `flui-hot-reload` is one, which an
  application adds itself and the framework reaches only through `DevReloadHook`;
- a crate's invariants are in its `ARCHITECTURE.md` (in the crate's `docs/` for
  `flui-animation`, `flui-assets`, `flui-interaction`).

## Working here

Many agents work in parallel and across sessions, so state lives in the repository, not in a
conversation:

- A feature runs on its owner-approved spec in `docs/plans/specs/<feature>/` (`requirements.md`,
  `design.md`, `tasks.md`). `tasks.md` is where the next session resumes: its status line, the
  checked tasks and the validation evidence. Requirement and task IDs live only there. Once the
  feature is on `main`, its spec is deleted.
- A decision that outlives the task goes into an ADR or the crate's `ARCHITECTURE.md`. Plans,
  roadmaps, research, reviews and session notes stay out of the public repository.
- **Build what is accepted before designing further.** `docs/gaps.md` lists the accepted ADR
  sections the code does not follow yet
  ([Accepted ADR sections not built yet](docs/gaps.md#accepted-adr-sections-not-built-yet)).
  While ADR-0304, ADR-0305 or ADR-0306 has a bullet there, a change to `flui-layer`,
  `flui-rendering`, `flui-painting` or `flui-engine` builds a task of
  `docs/plans/specs/render-scaling/` or fixes a
  defect, and it writes a new ADR for that pipeline only when the step it builds cannot be built
  without one. While `docs/gaps.md` has
  [An interactive widget costs eighteen elements](docs/gaps.md#an-interactive-widget-costs-eighteen-elements),
  no new widget family starts; a family under way is finished.
- **A replacement finishes in its own change.** A PR that brings a form to replace another (a
  primitive, an API, a rule, an ADR decision) moves every user of the old form and deletes it, or
  marks it `#[deprecated(note = "use …")]` and adds a `docs/gaps.md` entry for what is left.
  Every user includes the documents: `rg` the old name and the old rule across `crates/`,
  `packages/`, `docs/`, each `ARCHITECTURE.md` and this file, and correct them in the same PR.
- **A document states the current rule.** When a rule changes, the ADR, `ARCHITECTURE.md` or spec
  that states it is rewritten, not appended to: no "Amended" line over an unchanged body, no dated
  session entries, no progress diary. History is git's. A spec's `tasks.md` is a status table
  with its validation evidence, and the spec closes when its code and headless tests are on
  `main`; runs still owed on devices become rows of `crates/flui-native/platform-parity.toml`.
- **One source per fact.** A count, a status, a version, a crate list or a feature table lives in
  one place (a manifest, `platform-support.toml`, `docs/gaps.md`) and other documents link it.
  A PR that changes a fact someone copied fixes each copy.
- A consumer-visible change adds `changelog.d/<branch-slug>.md`
  (`### Added|Changed|Deprecated|Removed|Fixed|Security` and bullets), so parallel PRs never
  edit `CHANGELOG.md`. A platform-support change updates `crates/flui-native/platform-parity.toml`
  and restates `docs/platform-parity.md` from it (✅ only
  after a run on that platform) and `docs/gaps.md`.
- Commits are `area: what changed`. Pull-request CI executes Linux and wasm32 only; Windows,
  macOS, the iOS Simulator and an Android emulator run nightly on main
  (`nightly-platforms.yml`), so native and GPU changes are run locally before the PR.
- A job only main runs has a narrow form, which a pull request runs when its diff reaches what the
  job reads: a row of `ON_DEMAND` in `tools/xtask/src/change_scope/aggregator.rs`, decided by
  `cargo xtask affected` (ADR-0303 §9). `test-trybuild` runs when the diff touches a file a
  `*.stderr` cites, a case, a runner, a procedural macro or a message an expectation copies,
  `feature-scope` runs over the changed crates' features, `size` runs when the diff reaches a
  manifest, `Cargo.lock`, the toolchain, a `flui create` template or `perf/size.toml`,
  `send-positions` runs when a changed line of the facade's crates names `Send`, `Sync`, a
  `pub use`/`pub mod` or a listed item, and `miri` runs when a changed file sits under a source
  it interprets whole. A new main-only job that a pull request can break gets such a row.
- No process markers (`Cycle N`, `PR #NNN review`, `Phase B`, slice labels) in code or docs.
  Archival roots are exempt (`docs/plans`). That root holds the live specs, and their task IDs
  are what a spec tracks.
- No Python, Perl or PowerShell scripts for edits.

## Commands

Cargo runs through heli: `cargo heli <cargo arguments>`, repository tasks as
`cargo heli xtask <task>`.

### Checking a change

Before a PR: `cargo fmt --all`, then heli's clippy (`--all-targets -- -D warnings`) and nextest
on the changed crates, and their dependents when a public API changed. A docs-only change needs
only `cargo heli xtask checks`. `flui-native`'s suite runs only on Linux, so a Windows or macOS
host change needs `cargo heli nextest run -p flui-native` by hand.

- Each worktree keeps its own `target/`: a shared `CARGO_TARGET_DIR` links another worktree's
  sources.
- nextest skips doctests (`cargo test --doc`); GPU readback suites stay at one test thread.
- A flaky test usually touches process-global state (`flui-log`'s subscriber,
  `flui-foundation`'s key counters): lock that test module.
- Pre-1.0 the MSRV tracks latest stable (`rust-toolchain.toml`).

## What the compiler and gates enforce

| Rule | Enforced by |
|------|-------------|
| Presentation capabilities (`rebuild_handle`, `focus_manager`, `text_input_handle`, `pipeline_owner`, …) and signals are acquired only in `create_state`/`init_state`/`did_change_dependencies` | `LifecycleContext`, which only those hooks receive (ADR-0078) |
| Signals are read in `build`, never written or created there | run-time guard in `flui-view::reactive` (ADR-0074) |
| **ID offset** — plain slab-backed IDs (`ViewId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize` (`slab_index + 1`); generational keys (`ElementId`, `RenderId`, `UiRuntimeId`) are minted with `new_gen(slab_index, generation)` and read with `.index()` | ID newtypes |
| Logical and device geometry don't mix (ADR-0098) | trybuild `crates/flui-painting/tests/compile_fail/` |
| No bare `unwrap()` in production: `expect("BUG: <invariant>")`, `thiserror` in libraries, `anyhow` in apps ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)) | clippy |
| A public enum is `#[non_exhaustive]` unless its closure is a decision, stated by `#[expect(clippy::exhaustive_enums, reason = "…")]` on it | clippy `exhaustive_enums` (ADR-0329) |
| A caught panic payload nobody resumes goes to `discard_payload`; `mem::forget` is a deliberate leak with its reason at the call | clippy `mem_forget` |
| Dependencies point down the tiers; an exception names the ADR that removes it | `xtask workspace`, `xtask reach` (ADR-0081) |
| A Stable crate's public API names upstream types only as ADR-0089 §2 allows | `xtask api-closure` (CI `api-contracts` job, nightly rustdoc JSON) |
| Every item the facade reaches is in `api/flui.txt`, so a pull request that changes the facade's API carries the diff (`--write` restates it) | `xtask public-api` (CI `public-api` job, nightly rustdoc JSON; ADR-0324) |
| A pull request that edits an expectation (the API snapshot, a `*.stderr`, a `*.snap`, a protocol schema, a perf baseline, an allowlist, a test, a `deny.toml` or `typos.toml` value) names each path and its reason under `## Expectations` in its body | `xtask expectations --body` (`expectations.yml`, on every pull request and every edit of its body; ADR-0331) |
| `docs/platform-parity.md` is the render of `crates/flui-native/platform-parity.toml` (`FLUI_OVERWRITE=1` restates it), and a row of a position a host fills with `Absent` claims no support there | flui-native's `parity_document_matches_its_source` and `absent_positions_claim_no_support` (ADR-0303 §2) |
| Every `static`/`thread_local!` outside tests has a `globals` entry (an ADR that removes it, or an ADR-0097 grant) | `xtask globals` |
| Every OS entry into `flui-native` (an `extern` fn, an Objective-C or COM method, an AccessKit handler, a closure handed to the OS) is a constant or one call of `owner::panic_boundary::os_entry`, or of an adapter of its file that is (ADR-0151 §16) | `xtask panic-boundary` |
| A value crosses a thread because the compiler proves it may: every `unsafe impl Send`/`Sync` is listed and the list only shrinks; `send_wrapper`-style crates are banned; every `Send`/`Sync` position an author wrote in the facade's API is listed under ADR-0323 §3 | `xtask send-impls`, `deny.toml`, `xtask send-positions` (CI `api-contracts` job, nightly rustdoc JSON) |
| Test support is named `testing`; a hand-written `pump`/`advance`/`tick` or a `Mock*`/`Fake*`/`Scripted*`/`Recording*`/`Probe*` type in test code is in a kit or counted in the allowlist (ADR-0243) | `xtask testing` |
| A constructor that can fail is `try_new`: every `new` returning `Result` or `Option` is listed and the list only shrinks | `xtask try-new` |
| A context is bound as `ctx`, never `cx`, whatever its kind | `xtask context-names` (ADR-0325) |
| A description type's builder method (a view, `Application`, a `…Config/Options/Settings/Request/Builder/Params/Spec`) has the bare name, never `with_`; the list of the ones left only shrinks | `xtask builder-names` (ADR-0325) |
| A production `expect` or `panic!` names its invariant (`BUG: `) or its `try_` twin; the per-file counts of the ones that do not only go down | `xtask panic-messages` |
| User code runs through `flui_foundation::panic` (`commit`, `FirstPanic`) and an OS entry through `os_entry`: `std::panic::catch_unwind` is a disallowed method, and a site that still catches by hand carries `#[expect(clippy::disallowed_methods, reason = "ADR-0287: …")]` on its statement or function (ADR-0303 §6) | clippy (`disallowed-methods`) |

## ADR Policy

ADRs (`docs/adr/`) record the cross-crate decisions the code follows, in the MADR shape
[`docs/adr/README.md`](docs/adr/README.md) gives: drivers, every option with its pros and cons,
the decision as a Rust shape, and how compliance is confirmed. A change of decision is a new ADR
with `Supersedes:`, and `Superseded-by:` on the old one; code that disagrees with an accepted ADR
is a defect in one of them. A reversal reaches, in the same PR, every record that still states the
old rule (other ADRs, specs, each `ARCHITECTURE.md`, `docs/`), and a `Proposed` ADR supersedes
nothing until it is accepted. When a topic has gathered several partial supersessions, the next
change to it writes one ADR that states the whole current rule and supersedes the chain.

## Extending FLUI

Adding a render object, widget, text-editing widget, platform capability, crate, official
package or a `material`/`cupertino` example: [`docs/extending.md`](docs/extending.md) has the
steps.

## Definition of Done

New behavior has a test that fails without it, written through the public API from `tests/`. In
`flui-native` the tests of a feature's model and of a platform file's pure part sit beside them as
`pub(crate)` modules and run on the Linux CI host; its `tests/` keeps the conformance table and the
recordings of native traces. Every concrete `RenderBox`/`RenderSliver` has a
`render_object_harness` row. Cases that differ only in input join an existing table test, and a
crate's integration tests are modules of one `tests/main.rs`. Tests are cited by name in docs and ADRs: `rg` before renaming one.

## Before each push

Run this over your own diff before every push, a review fix included. Each line is a question
with an answer you can point to in the code; the reviewers ask the same ones.

1. **User code.** List every point where the diff runs code it does not own: a callback, a
   wake, a waker's `clone` or `drop`, the `Drop` of a replaced or captured value, `Into`,
   `Display`, a `tracing` event (a subscriber is user code). At each one, are borrows and locks
   released and the state committed?
2. **A panic at each of those points.** What state is left, which flag stays set, does the next
   operation still work, is the first failure kept when two points panic? A destructor that
   panics while the thread is already unwinding aborts, as in std (ADR-0281): no guard for it.
3. **Accepted work.** Every request, waker, obligation and hold the diff accepts is either
   delivered or refused with a typed error on every exit: success, panic, cancel, owner
   retirement, a full queue.
4. **Asynchronous answers.** An answer is matched to its request by a ticket or generation, not
   by "the current one". Check an answer that arrives late, after a write, or after focus or the
   owner moved.
5. **Identity.** A cache or coalescing key holds everything the result depends on (client, seat,
   document, version), compared by typed ID.
6. **Untrusted input.** Size checked before decoding or copying, how many copies of one payload
   live at once, every queue and cache bounded.
7. **Parity.** Policy (a limit, normalization, a version floor) lives at the seam. If a backend
   holds it, every backend has it, headless included.
8. **Mirrors.** A diff that syncs a second copy of state lists every field of the source and
   every operation that changes it: reorder, removal, hiding, reversed direction, the browser's
   own normalization.
9. **Arithmetic.** No overflow in intermediate values, no non-finite geometry published, units,
   clocks and 0- versus 1-based indexes match.
10. **The test fails without the fix.** Stash the fix, run the new test, see it fail; it goes
    through the public API.
11. **Repository rules.** A generic-return call names its target type, a new `pub` item has a
    production caller or a named follow-up, no requirement or task IDs in code, `docs/gaps.md`
    and `crates/flui-native/platform-parity.toml` updated with a platform change.
12. **A review finding fixes its class.** Apply items 1–3 to every similar point in the PR, not
    only the line the reviewer named.
13. **Replaced forms.** What does the diff replace or reverse? Name the old form, `rg` it across
    code and documents, and point to zero users left, or to the deprecation and its
    `docs/gaps.md` entry. Does any new line use a form from the "Not" column of "Current forms"?
14. **Expectations.** `cargo xtask expectations` lists what the diff edits of what the checks
    compare against: lines gone from `api/flui.txt`, a changed `*.stderr`, `*.snap` or protocol
    schema, a raised perf baseline, a grown allowlist, fewer tests, a new `#[ignore]`, a new
    `deny.toml` or `typos.toml` value. For each, was the old expectation wrong, or is the code?
    An expectation moved to turn a check green is the defect the check was there to find. The
    pull request's body names each path under `## Expectations` with that answer (ADR-0331).

## Before a merge

The author merges a pull request once CI is green, so the author's reading is the only one a
change is sure to get, and it is the one least able to find what is wrong: the list above is
answered by the reasoning that wrote the code. Before the merge, a reviewer that did not write
the change reads it to break it:

- **A fresh context.** A subagent or a separate session that has seen none of the author's
  conversation. It gets the diff against the merge base, the list `cargo xtask expectations`
  prints and the checkout; not the plan, not the PR body, not the author's account of why the
  change is right.
- **One job.** It assumes the change has a defect and looks for the input, the order of events
  or the state that shows it, in the code around the diff as much as in the changed lines,
  working from "Recurring defects", "Before each push" and "Review guidelines". It does not fix,
  and it does not sum up what is good.
- **A finding is a failure scenario**, in the form [`REVIEW.md`](REVIEW.md) asks for: the state,
  the operation, the wrong result, the `file:line`. A reviewer that finds none names the
  scenarios it tried.
- **The author answers each finding** under `## Review` in the PR body: fixed, with its class
  (item 12), or refuted by the line or the test that shows the scenario cannot happen. That it
  is unlikely is not a refutation.
- **A fix is read again.** The review that found a defect has not seen its fix: a change pushed
  after the review gets the same reading of its own diff.

The implementer does not review and the reviewer does not implement. A pull request the `docs`
lane takes (documentation only) skips the step; its body says so under `## Review`.

## Where to read next

`docs/architecture.md` (the code as it is), `docs/adr/` (the decisions), `docs/testing.md` (test
tiers), `docs/platform-parity.md` and `docs/gaps.md`, [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Review guidelines

Pull request reviewers (Codex, Claude Code Review, a local `/code-review`, the reader of "Before a
merge") work from this section; [`REVIEW.md`](REVIEW.md) sets severity and scope for Claude Code
Review. Report defects with a concrete failure scenario, and code an experienced Rust developer
would be embarrassed by, with the better shape. What this repository keeps getting wrong:

- a new `pub` item that no production path reaches, unless the PR names the follow-up;
- an edited expectation (the list `cargo xtask expectations` prints) whose line under
  `## Expectations` in the PR body does not say why the old one was wrong: a snapshot
  regenerated, a test removed or a baseline raised because the check was red;
- the "Recurring defects" above, checked at each failure point alone, two in competition, and
  the next operation after containment ([matrix](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/signal-unwind-contract.ru.md));
- `SliverGeometry { ..SliverGeometry::ZERO }` (drops `layout_extent`, `visible`), geometry from a
  stand-in value (ADR-0054), intrinsics, baselines or hit-testing left at defaults;
- a pixel claim whose readback samples don't distinguish fixed from broken; a `wgpu::Instance`
  created apart from its surface;
- state owned by the process instead of a UI runtime; event translation tested only
  synthetically;
- shapes transliterated from another framework (Dart-style names, setter/getter pairs);
- a `type` alias for values that are easy to confuse, where a newtype makes the mix-up a type
  error; a `new` returning `Result` or `Option` (that constructor is `try_new`);
- a generic-return call (`.into()`, `.parse()`, `collect()`) whose target type the reader has to
  infer: name it with a turbofish or a typed binding;
- `SAFETY:` comments that restate the operation instead of naming the invariant;
- workflow actions not pinned to a full SHA, cargo without `--locked`, caches saved off `main`;
- an exemption (`deny.toml` skip, `typos.toml` word, `#[expect]`) without its reason;
- a second way to do something that already has one, with the first left undeprecated, or a new
  use of a form from the "Not" column of "Current forms";
- a document that records a change by appending to a record instead of rewriting its rule, or a
  reversal that leaves other records teaching the old rule.

Not a finding: a native or GPU path not yet run on its platform. PR CI runs Linux and wasm32 only;
the PR body names the platforms the change still needs a run on, and the owner runs them from a
device list.
