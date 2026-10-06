# AGENTS.md

The one guide for every agent runtime and human contributor (`CLAUDE.md` just imports it). It
records what isn't derivable from the code: the project's design stance, the rules the compiler
and gates enforce, and the conventions they can't check.

---

## What FLUI is

A declarative UI framework for Rust, inspired by Flutter's widget-style composition and
designed for Rust everywhere else. Five trees — `View` (immutable config) → `Element` (lifecycle, reconciliation)
→ `RenderObject` (layout / paint / hit-test) → `Layer` (compositing, rebuilt each frame), with
`Semantics` alongside for accessibility — then `flui-engine` → `wgpu`. Pre-1.0: breaking changes
are cheap now and expensive once consumers exist, so fix a bad shape instead of working around it.

## Design stance

- **FLUI is not a Flutter port.** What it took from Flutter: widgets composed as declarative
  Rust values (not markup), the View/Element/RenderObject separation, constraints-down/sizes-up
  layout. Everything else is decided by asking what the best design is here; "Flutter does it
  this way" is never the reason on its own, and existing code that differs from Flutter is
  usually a deliberate choice, not a bug to "fix" toward parity. Structure, API and style are
  idiomatic Rust (compile-time child arity, `NonZeroUsize` IDs, slab arenas,
  `thiserror`/`Result`, builders and typed state, not Dart class chains or mutable-field
  setters). Multi-window ownership, runtime/scheduling topology, concurrency and presentation
  architecture aren't bound by Flutter at all (ADR-0027).
- **Other frameworks are a checklist, not a target.** Flutter's tests and edge cases show what a
  behavior must cope with; Compose, SwiftUI and the Rust UI crates (egui, Iced, Xilem/Masonry,
  Bevy UI, GPUI, Dioxus, Slint) often have the better shape, and where Flutter has no strong
  contract (animation curves, velocity prediction, color interpolation, input smoothing) it
  isn't the baseline at all. Read them after you have a design, to check it. Pin the behavior
  you ship with a test; a cross-crate contract also gets an ADR. Prefer a mature crate over a
  hand-rolled one. No copy is checked in: use `gh` (`gh search code --repo flutter/flutter
  <term>`, `gh api repos/flutter/flutter/contents/<path> -H "Accept: application/vnd.github.raw"`;
  GPUI is `crates/gpui` in `zed-industries/zed`, Xilem is `linebender/xilem`, egui is
  `emilk/egui`), or a shallow clone into the gitignored `.flutter/` or `.gpui/`, which may not
  exist yet.
- **Make rules types, not reviews.** If the compiler can reject a mistake, encode it (arity
  types, sealed traits, `LifecycleContext`); if clippy can, turn the lint on; a comment or a
  grep is the last resort.
- **Frame path is synchronous.** No `async` in build/layout/paint; async lives at IO, scheduler
  and tooling edges, and delivers results to the next frame. Locks guard shared infrastructure
  only: a lock on per-node state touched inside `perform_layout`/`paint` puts contention (and a
  deadlock risk) on every frame, and a lock in a public signature makes callers part of the
  locking protocol.
- **Layers, not micro-crates.** A crate is a layer with one-way dependencies; a feature inside a
  layer is a module (`flui-widgets` stays one crate). Shared code moves down a layer, not
  sideways into a copy.

## Recurring defects

Each of these has shipped more than once; design against them up front.

- **Treat user code as reentrant.** Callbacks, wakers, observers, diagnostics and generic
  `Drop` implementations can access the same subsystem, replace a hook or release its last
  owner. Commit internal state and move outgoing ownership out while guarded; release lock
  guards and `RefCell` borrows before invoking or retiring user code. Test reentry through
  the same public handle, including replacement and owner release.
- **Catching a panic does not finish cleanup.** At a containment boundary, account for
  callback captures, old and rejected values, panic payloads and nested cancellation. A caught
  panic makes `thread::panicking()` false; preserve the first failure explicitly across nested
  retirement until recovery finishes. Establish the subsystem's documented ownership policy
  before invoking diagnostics or recovery code. Use its existing containment helpers, keep
  healthy-path destruction, and state the limits: an outer catch cannot rescue an aggregate
  whose destructors already double-panic before reaching it.
- **Accepted work must remain deliverable.** Commit admission before waking, and represent
  pending delivery separately from queued data. A missing, replaced or panicking hook must not
  erase delivery debt; an older successful wake must not clear a newer obligation. On failure,
  preserve the accepted tail and arrange a retry according to the subsystem's contract. Test
  recovery, repeated IDs and independent handles sharing that state.
- **Identity is not a label.** Compare backend handles or typed identities, never display
  names or model strings. Specify the absent-owner fallback. Keep ownership and generation
  checks at lookup and retirement; exhausting an ID must refuse permanently instead of
  wrapping or reissuing a stale identity. Cover duplicate labels, stale handles and the
  terminal counter boundary where applicable.
- **Check intermediate arithmetic and the final output.** Finite inputs can overflow during
  multiplication, squaring or inversion. Define the admitted range and degeneracy behavior;
  preserve meaningful results without publishing non-finite geometry. Distinguish text's
  wrapping limit from its allocated width, source-image crop from sampling bounds, and theme
  defaults from explicit widget configuration. Test loose and tight constraints, RTL,
  fractional boundaries and degenerate cases through the actual affected producer.
- **Regression tests that pass both ways.** For a behavior repair, run the case with the fix
  reverted; it must fail for the intended reason. Revert in an isolated checkout, or restore the
  exact original bytes afterwards, and never while another build reads the sources. Pixel
  samples must distinguish the two outputs; a changed snapshot is explained, then re-run with
  snapshot updates off.
- **Green defaults are not coverage.** A workspace-wide change or audit covers optional
  features, tests, examples, shaders and tooling too, and says which platform and feature paths
  were only compiled, or not run at all, on this host.

## Codebase map

24 crates under `crates/`, official packages under `packages/`, and the `flui` facade (`src/`),
strictly layered; each manifest's `[package.metadata.flui]` declares its tier and layer, and
`docs/crates.md` is the readable map. Bottom to top: values (`flui-macros`, `flui-foundation`
with its `f64` geometry, ADR-0098) → contracts (`flui-platform-api`, `flui-protocol`) →
substrate (`flui-platform`, `flui-scheduler`, `flui-painting`, `flui-interaction`,
`flui-assets`, `flui-log`) → compositing (`flui-layer`, `flui-semantics`, `flui-animation`) →
render machine (`flui-rendering` protocols, `flui-objects` catalog, `flui-engine` → `wgpu`) →
spine (`flui-view`, `flui-widgets`, `flui-runtime`, `flui-sdk`, `flui-testing`) → packages →
composition roots (`flui-app`, `flui-cli`, the facade). What the layout doesn't tell you:

- Only `flui-app` depends on `flui-platform`, and every `windows::*`/`objc2::*` type stays
  inside it (ADR-0082).
- `flui-runtime` is moving out of `flui-app` (ADR-0083); it has no host, platform or GPU edge.
- `flui-sdk` is the Evolving package-author surface, versioned `0.N` apart from the train
  (ADR-0088). Official packages (`flui-material`, `flui-cupertino`, `flui-devtools`) build on it
  alone, as a third-party package would. `flui-hot-reload` is an official package still under
  `crates/`, reached from `flui-app` only through `DevReloadHook` (ADR-0094 §1).

The non-obvious invariants live in the per-crate `ARCHITECTURE.md` files — read the one for the
crate you're changing before changing it (`flui-animation`, `flui-assets`, `flui-interaction`,
`flui-log` and `flui-protocol` have none yet: their module docs and `docs/crates.md` stand in).
The root `ARCHITECTURE.md` is the facade's. `docs/architecture.md` describes the code as it is;
`design/` holds the target architecture, and
`docs/plans/2026-09-25-architecture-migration-plan.md` orders the steps toward it.

## Working here

- **Isolate each task in its own worktree**; the shared checkout stays on `main`:
  `cargo xtask worktree new <area>/<slug>` creates `.worktrees/<slug>` inside the checkout
  (git-ignored; the gates skip it). Each worktree builds into its own multi-GB `target/`; never
  point several worktrees at one `CARGO_TARGET_DIR` (Cargo then links another worktree's
  sources — `docs/testing.md`, "One target directory per checkout"). Run
  `cargo xtask worktree prune` after a merge. Review someone else's PR from your own directory
  (`gh pr diff`/`checkout`), not inside their worktree.
- **Commits** `area: what changed`, one logical change each. **PRs** are one task each, with
  `cargo xtask check-changed` green first; CI is the proof. Review your own branch against
  `main` before asking for review. CI runs Linux (and wasm32) only; Windows, macOS, Android and
  iOS code is type-checked by `cross-typecheck` and never executed there, and no label enables
  native or GPU jobs, so run the platform or GPU commands locally when a change needs them. Use
  `Refs #N`; `Closes`/`Fixes #N` only when merging should close it (GitHub's linker ignores
  negation around it). A consumer-visible change adds `changelog.d/<branch-slug>.md` (a
  `### Added|Changed|Deprecated|Removed|Fixed|Security` header and bullets) instead of editing
  `CHANGELOG.md`; `cargo xtask changelog --write` merges fragments at release.
- **Red main:** fix forward within the hour, or revert. A red CI run on main or nightly opens a
  "CI is red on main" issue; close it once main is green.
- **Leave these alone unless the task is about them:** `.github/workflows/` (it is the merge
  path, and a change there decides what every other PR must pass); `Cargo.lock` by hand (cargo
  regenerates it, a hand edit drifts from the manifests).
- **No internal process-ID markers** (`Cycle N`, `PR #NNN review`, `Phase B`, slice/wave labels)
  in code or docs — state the invariant, not the history that produced it. `ADR-NNNN` citations
  are fine. Archival roots are exempt (`docs/{plans,research}`).
- **A new gate** is a `cargo xtask` command *and* a step in a CI job the `ci` aggregator gates,
  usually `checks` (a check folded into `cargo xtask checks` gets both) — a command alone never
  reaches the merge path. Prefer a lint or a type over a new check. A doc pulled in with
  `include_str!` is source: keep it out of `DOCS_ONLY` in
  `tools/xtask/src/change_scope/classify.rs`.

## Autonomy

The maintainer usually hands over a whole task and comes back later, so carry it through
without check-ins. Ask first only before something irreversible or outward-facing: deleting
data, force-pushing, merging, publishing, or changing anything outside your worktree.
`TASKS.md` at the worktree root is git-ignored scratch for a long task's checklist.

The repository is public. Plans, reviews, audits and session notes stay out of it (keep them in
`TASKS.md` or outside the checkout); a decision that should outlive the task goes into an ADR,
`design/`, or the crate's `## Mapping decisions`. Never commit local absolute paths or links to
private chat sessions. The exception is `docs/plans/specs/<feature>/` (`requirements.md`,
`design.md`, `tasks.md`): the owner-approved feature specs the release work runs on, with their
status. Requirement and task IDs live only there; once a feature merges, its lasting decisions
move into an ADR or the crate's `ARCHITECTURE.md`, and the spec stays as history.

## Commands

`cargo xtask --help` lists every repository task (crate `tools/xtask`); anything else is plain
`cargo`. The ones you need most:

| Need | Run |
|------|-----|
| Before a PR | `cargo xtask check-changed` — fmt, clippy and nextest over the changed crates and their dependents, classified the way CI's `plan` does |
| Full local gate | `cargo xtask ci` (`gate` + `test` + doctests); `cargo xtask ci-full` adds the heavy jobs, `cargo xtask doctor full` names missing tools |
| One crate / one test | `cargo nextest run -p <crate> [<test>] --no-capture` |
| Render-object catalog | `cargo test -p flui-objects --test render_object_harness` |

Gotchas: nextest doesn't run doctests (`cargo test --doc`). A flaky test that isn't yours usually
touches genuinely process-global state (the global `tracing` subscriber `flui-log` installs, a
global ID counter such as `flui-foundation`'s key counters) — scope a lock to that test module
rather than serializing the suite. A docs-only change needs only `cargo xtask checks`, which
builds xtask and not the workspace. `rust-toolchain.toml` is the toolchain's source of truth;
pre-1.0 the MSRV tracks latest stable.

### Running checks without fighting other runs

Several agents and checkouts often share one machine. Every redundant run slows down every other
run, and an oversubscribed host makes slow tests look hung.

- **While iterating, test only what you touched:** `cargo nextest run -p <crate> [<filter>]`.
  Run `cargo xtask check-changed` once, as the last step before a PR. It already runs fmt, clippy
  and nextest, so don't also run them by hand.
- **Don't re-run a gate that passed** unless the code changed since. Quote the earlier result
  instead.
- **One heavy run at a time per host.** `check-changed`, `test`, `ci`, `gate` and `gpu-test` take a
  host-wide lock and queue behind each other. Don't start a second one in the background to "save
  time", and don't kill a queued run.
- **Cap parallelism on a shared host:** `CARGO_BUILD_JOBS=6` and `NEXTEST_TEST_THREADS=4`. GPU
  readback suites stay at one test thread. In nextest, `-j` sets test threads; use `--build-jobs`
  for the build.
- **A test past its `slow-timeout` on a loaded host is not a hang by default.** Before calling it
  a bug, re-run that one test alone (`--test-threads 1`) and report how long it took.

## What the compiler and gates enforce

The gates explain their own findings; this is what to design for up front.

| Rule | Enforced by |
|------|-------------|
| Presentation capabilities (`rebuild_handle`, `focus_manager`, `text_input_handle`, `pipeline_owner`, …) are acquired only in `init_state`/`did_change_dependencies` | type system: they live on `LifecycleContext`, which only those hooks receive (ADR-0078) |
| Signals are read in `build`, never written or created there | run-time guard in `flui-view::reactive` (ADR-0074) |
| **ID offset** — slab indices are 0-based. Plain slab-backed IDs (`ViewId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`: insert `slab_index + 1`, look up `id.get() - 1`. Generational keys (`ElementId`, `RenderId`, `RealmId`) pack the 0-based slot and a non-zero generation: mint with `new_gen(slab_index, generation)`, read `.index()` | `NonZeroUsize` / `NonZeroU64` + ID newtypes |
| Logical and device geometry don't mix (no `Point + Point`, no `Size` as an `Offset`, no `DevicePoint` as a `Point`, no `f64`/`i32` mixing; ADR-0098) | trybuild suite `crates/flui-painting/tests/compile_fail/` |
| No bare `unwrap()` in production: `expect("BUG: <invariant>")` for internal invariants, `thiserror` in libraries, `anyhow` in apps ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)); no `todo!`/`dbg!`; no lock guard held across an `if let`/`match` | clippy (`unwrap_used`, `significant_drop_in_scrutinee`, …) |
| Dependencies point down the tiers declared in `[package.metadata.flui]`; an exception names the ADR that removes it. Official packages depend on `flui-sdk` and the contract crates only | `cargo xtask workspace`, `cargo xtask reach` (ADR-0081, ADR-0088) |
| `flui-widgets` modules import only lower modules | `cargo xtask module-dag` |
| Every `static`/`thread_local!` outside tests has a `globals` entry: an ADR that removes it, or an ADR-0097 grant | `cargo xtask globals` |
| No unused or test-only dependency in `[dependencies]`; licenses and advisories per `deny.toml` | `cargo xtask deps` |
| No process markers (see "Working here"); at most 3000 production lines per `.rs` file; doc links and repository paths resolve; `changelog.d/` fragments are well-formed | `cargo xtask checks` (`markers`, `file-length`, `docs-links`, `docs-paths`, `changelog`); their allowlists under `tools/xtask/allowlists/` only shrink |

## ADR Policy

ADRs (`docs/adr/`) record cross-crate decisions the code still follows. Revise them freely but
explicitly: a new ADR with `Supersedes: ADR-XXXX`, and `Superseded-by: ADR-YYYY` added to the old
one. Code that silently disagrees with an accepted ADR is a defect — either the code is wrong or
the ADR needs superseding. Delete an ADR whose decision no longer exists in the code; git keeps
the history.

## Extending FLUI

| Adding | What it takes |
|--------|---------------|
| **Render object** (`RenderBox`/`RenderSliver`) | Implement in `flui-objects` (protocol in `flui-rendering`) → register in `RENDER_OBJECT_TYPES` → `harness_*` tests in `render_object_harness` → record a non-obvious decision in the crate's `## Mapping decisions` |
| **Widget** | `View`/`ViewState` in `flui-widgets` or the facade, backed by a render object → `SemanticsConfiguration` for assistive tech → a test that fails without it |
| **Text-editing widget** | Implement `flui_platform_api::TextStore` (embed a `LockArbiter` and pass it the `CommitGate` that `set_commit_gate` receives; the store keeps no transaction flag of its own), attach it through `TextInputHandle::attach` while focused, which installs the presentation's frame-transaction gate → pass `flui_testing::text_store_kit::assert_conforms` (ADR-0090) |
| **Platform capability** (a new handle) | Trait in `flui-platform-api`, backend in `flui-platform` with no platform types leaking out → a method on `LifecycleContext`, not `BuildContext`, so `build` cannot reach it → a test that fails without it → ADR if it changes a cross-crate contract |
| **Crate** | A workspace `members` entry and `[package.metadata.flui]` `tier`, `tier-kind`, `order` and `layer = N` (names: root `[workspace.metadata.flui] tiers` and `layers`); `cargo xtask workspace` checks the rest. Why a crate must be a layer: `docs/crates.md` "Adding a New Crate", [ADR-0041](docs/adr/ADR-0041-workspace-topology-contract.md) |
| **Official package** | Under `packages/<name>/`, `tier = "pkg"`, `tier-kind = "official"`, no `edge-exceptions`; its only FLUI normal dependency is `flui-sdk` (plus the contract crates), and its code names the framework as `flui_sdk::…` (the view, inherited and animation derives resolve through it; `Diagnosticable`'s has no SDK path yet). An item the SDK lacks is added to `flui-sdk` by ADR-0088 §4, with a line in its `tests/surface.rs` pinned list |
| **Example using `material`/`cupertino`** | `[[example]] required-features = [...]` (`cargo xtask facade-combos` relies on it) |

## Writing tests

A test earns its place by pinning a contract, not a structure. The suite is small on purpose (a
few hundred tests), so a new case usually joins an existing table rather than adding a test.

- **Test through the public API**, from `tests/`. An in-`src` `mod tests` is only for what a
  consumer cannot reach (a failure-path matrix needing a private seam, a `## Mapping decisions`
  entry); compile-fail cases are trybuild fixtures driven from `tests/`. Don't pin private
  fields, dirty flags, `size_of`, internal constants or `Default`/`Debug`/getter round trips: a
  refactor that keeps behavior must not touch a test. Values a document fixes (ADR-pinned wire
  spellings, ABI) are contract and keep their tests.
- **A family is one table.** Cases that differ only in input are rows of one table-driven
  `#[test]`, each row a plain `fn` named after the case, run with the crate's existing runner
  (`table_test::run_table`, `test_cases::run_cases`, `tests/contracts.rs`). Only the runners in
  `flui-foundation` and `flui-animation` contain a panic payload whose `Drop` panics; elsewhere a
  row must not throw one.
- **Few binaries.** Each root `tests/*.rs` is a binary that links the whole stack, so crates
  mount their integration tests as modules of one `tests/main.rs` (see `flui-widgets`'
  manifest); a new file is a new `mod` line. A separate target is only for process-global state
  or a feature the rest of the crate builds without. Tests that spawn a process (trybuild,
  nested `cargo`) stay separate: `.config/nextest.toml` names them so they run in parallel.
- **Keep what the Definition of Done requires**: a `render_object_harness` row for every
  concrete `RenderBox`/`RenderSliver` (checked against `RENDER_OBJECT_TYPES`), the test named by
  each `## Mapping decisions` entry, and a failure-path matrix's single, competing and
  after-containment cases.
- **Test names are references.** ARCHITECTURE.md files, ADRs and `docs/` cite tests by name:
  `rg` the name before renaming, folding or deleting one.

A test written on Windows can be red in CI on Linux or wasm32. `cargo clippy --all-targets
--target x86_64-unknown-linux-gnu` (or `aarch64-apple-darwin`) type-checks unix test code
without linking, except in crates with a C build script; `cargo xtask wasm-check` covers wasm.

## Definition of Done

A green gate proves the gates pass, not that the behavior exists. So a change is done when:

- new behavior has a test that fails without the change, and every concrete
  `RenderBox`/`RenderSliver` has harness tests;
- the behavior you ship is deliberate and asserted by a test; an edge case lost by accident is a
  regression, and so is a shape borrowed from another framework with no reason of its own.

## Where to read next

| Question | Read |
|----------|------|
| Is it planned? What changed recently? | `docs/ROADMAP.md`, `CHANGELOG.md` |
| Where the architecture is heading, open questions | `design/README.md`, `docs/plans/2026-09-25-architecture-migration-plan.md` |
| Dependencies, layering, a new crate | root `Cargo.toml` (`[workspace.metadata.flui] tiers` and `layers`, `[workspace.dependencies]`), `docs/crates.md` |
| Writing a frame-driving test | `docs/testing.md` (use the shallowest tier that can fail), `crates/flui-rendering/docs/TESTING.md` |
| Contracts, pipeline, panics | `docs/FOUNDATIONS.md`, `docs/architecture.md`, `docs/PANIC-POLICY.md` |
| Planning a large change, git hygiene | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Review guidelines

Pull requests are reviewed by Codex, which reads this section; a human reviewer can use it the
same way. fmt, clippy (pedantic, `unwrap_used`, the lints in the table above), rustdoc and the
script gates already run in CI, so formatting and anything they catch is not worth a comment.

- **What to report:** defects that would block the merge, each with a concrete failure
  scenario; without one, it is a hypothesis.
- **Code quality is a merge criterion, not style.** The bar is code an experienced Rust developer
  is not embarrassed by. Report, with a concrete better shape:
  - **Simplicity.** A second abstraction layer, generic parameter or trait with one user. A
    builder, newtype or enum is fine when it removes a mistake class.
  - **Ownership.** Moving instead of cloning; `Rc`/`Arc` only where ownership is really shared;
    `RefCell`/`Mutex` only where no `&mut` path exists. No borrow held across user code.
  - **Lifetimes and borrowing.** A borrowed view (`&str`, `&[T]`, `impl Iterator`) instead of an
    owned copy, with no lifetime gymnastics a reader has to decode.
  - **Generics, traits and GATs.** Static dispatch where the type is known, `dyn` where a
    heterogeneous collection or an object boundary needs it. Associated types and GATs over
    parameter soup. Sealed traits for closed sets.
  - **Types over conventions.** Illegal states unrepresentable (enums over flags plus options,
    typestate where it pays). Errors as `thiserror` enums a caller can match.
  - **Current stable Rust** (the toolchain in `rust-toolchain.toml`): let-chains, `let`-`else`,
    async closures, return-position `impl Trait` in traits, precise capturing, trait upcasting
    and current std APIs (`get_disjoint_mut`, `LazyLock`, …) where they make the code simpler.
    Check the release notes of the pinned version rather than recalling them.
  - **Conventions.** The Rust API Guidelines: naming, `as_`/`to_`/`into_`, getters without
    `get_`, `From`/`TryFrom`/`Display`/`Default` where they apply, `#[must_use]`,
    `#[non_exhaustive]` on public enums that will grow.
  - **Architecture.** One responsibility per module, dependencies down the layers, no
    behavior-free pass-through types.
- **Tests:** for each behavior change, find the test that covers it and ask whether it would fail
  with the production hunk reverted. Tests here have passed both ways by reimplementing the
  predicate they pin, asserting that a widget exists rather than that it was laid out or
  painted, pinning a `Send` bound with a type that already satisfies it, counting rebuilds
  through a harness helper that dirties the root itself, or narrowing an assertion to what a
  partial implementation handles. A regenerated `*.snap` is a claim the new output is correct —
  the PR must say what changed and why. A test that mutates genuinely process-global state
  (the global subscriber, a global ID counter) needs a module-scoped lock, because nextest runs one
  process per test in parallel.
- **Failure paths and recovery:** check the change against "Recurring defects" above. At each
  failure boundary, every owned value, guard, callback and deferred obligation must be
  accounted for; each failure point alone, two in competition, and the next operation after
  containment must leave the first failure authoritative and the subsystem making progress. See
  [`docs/research/signal-unwind-contract.ru.md`](docs/research/signal-unwind-contract.ru.md) for a
  concrete postmortem and regression matrix.
- **Unwired surface:** a new `pub` item that no production path reaches (test, example and
  bench callers don't count) is this repository's most common defect. Flag it unless the PR
  names the follow-up that wires it.
- **Behavior:** a change to render, layout, paint, hit-test, semantics, scheduling or
  reconciliation is judged on whether the result is right. The behavior you ship needs a test
  that fails without it. Flag a change that transliterates another framework (Dart-style
  names, class chains, setter/getter pairs, a copied private helper) when a Rust-shaped design
  would be simpler: a borrowed shape is not an improvement by itself.
- **Rendering specifics:** `SliverGeometry { ..SliverGeometry::ZERO }` drops the constructor's
  derived defaults (`layout_extent`, `visible`) and has caused real header bugs; a layout that
  publishes geometry from a stand-in value (ADR-0054); intrinsics, baselines or hit-testing left
  returning defaults while the PR calls the object done; a concrete render object missing from
  `RENDER_OBJECT_TYPES` or its `harness_*` test.
- **Engine:** a `wgpu::Instance` and the surface it must be compatible with are created
  together. A pixel claim needs a readback whose sample points distinguish the fixed code from
  the broken code — rotation about the centre, SSAA area gates and framebuffer rebases have each
  produced tests that passed both ways.
- **Runtime and platform:** state belongs to a realm (scheduler, focus, GlobalKeys), never to the
  process. Only the Linux/headless platform path executes in CI; Win32, AppKit, Android and iOS
  are clippy-only, so a change there is unverified unless the PR shows a run. Event-translation
  changes need the live smoke path, not a synthetic gesture test.
- **`unsafe`:** each block's `SAFETY:` comment names an invariant this code establishes, not a
  restatement of the operation; say so if a safe API would do.
- **Manifests and workflows:** shared dependencies go through `[workspace.dependencies]`;
  features stay additive and every optional dependency sits behind a `dep:` feature; a new crate
  declares its `[package.metadata.flui]` `tier`, `tier-kind`, `order` and `layer`, and
  `wasm = false` if it cannot build for wasm32. In workflows: actions pinned to a full SHA,
  `--locked` on every cargo call, caches saved only on `main`.
- **Registries and exemptions** (`RENDER_OBJECT_TYPES`, `docs/ROADMAP.md`, a `deny.toml` skip, a
  `typos.toml` word, an `#[expect]`): check that each entry matches the code in the same PR and
  that a new exemption states its reason.
- **Docs:** no process markers (see "Working here"); no hand-maintained completeness claims ("all
  call sites now use X") without the command that showed it; no claim of matching
  another framework without the reference it was checked against.
