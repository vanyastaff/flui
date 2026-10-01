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

- **FLUI is not a Flutter port.** Flutter is one source of ideas among several, not a spec.
  What we took from it: widgets composed as declarative Rust values (not markup or HTML-style
  templates), the View/Element/RenderObject separation, constraints-down/sizes-up layout. Every
  other decision is made fresh, by asking what the best design is here. Do not carry over Dart
  class hierarchies, method names, file layout, private helpers or a quirk just because Flutter
  has it; "Flutter does it this way" is never the reason on its own. Structure, API and style are
  idiomatic Rust (compile-time child arity, `NonZeroUsize` IDs, slab arenas,
  `thiserror`/`Result`, builders and typed state instead of Dart's mutable-field setters).
- **Flutter as a checklist, not a target.** Its tests and edge cases are useful for finding
  what a behavior must cope with; each one is kept, changed or dropped on purpose. Diverge
  whenever the result is better, and pin what you ship with a behavior test; a cross-crate
  contract still gets an ADR. Multi-window ownership,
  runtime/scheduling topology, concurrency and presentation architecture aren't bound by Flutter
  at all (ADR-0027). Read references after you have a design, to check it, not to copy from.
  There is no checked-in copy: read them on GitHub with `gh` (`gh search code --repo
  flutter/flutter <term>`, `gh api repos/flutter/flutter/contents/<path> -H "Accept:
  application/vnd.github.raw"`; GPUI is `crates/gpui` in `zed-industries/zed`, Xilem is
  `linebender/xilem`, egui is `emilk/egui`). `.flutter/` and `.gpui/` are gitignored, so a
  shallow clone there (`git clone --depth 1`) is fine when you need to grep a lot; never
  assume it exists.
- **Look around before settling.** Compose, SwiftUI and the Rust UI crates (egui, Iced,
  Xilem/Masonry, Bevy UI, GPUI, Dioxus, Slint) often have the better shape; where Flutter has no
  strong contract (animation curves, velocity prediction, color interpolation, input smoothing)
  it isn't the baseline at all. Prefer a mature crate over a hand-rolled one.
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

## Codebase map

24 crates under `crates/`, the official packages under `packages/`, and the `flui` facade
(`src/`), strictly layered. Each manifest
declares its tier and layer in `[package.metadata.flui]` (checked by `cargo xtask workspace`);
`docs/crates.md` is the readable version. Bottom to top:

- **Values & primitives** — `flui-macros` (View derives), `flui-foundation` (IDs, and the
  plain-`f64` geometry values in `flui_foundation::geometry`; ADR-0098).
- **Contracts** — `flui-platform-api` (platform contracts: capability traits and window/input
  vocabulary, no OS code; ADR-0082), `flui-protocol` (semantics roles and actions, the
  agent-protocol wire vocabulary; ADR-0095).
- **Substrate** — `flui-platform` (the backends behind those
  contracts: windows, input, IME, clipboard; every `windows::*`/`objc2::*` type stays inside it;
  only `flui-app` depends on it), `flui-scheduler` (frame phases),
  `flui-painting` (paint, styling and typography values; records into a `DisplayList`),
  `flui-interaction` (event routing, gestures),
  `flui-assets`, `flui-log`.
- **Compositing** — `flui-layer`, `flui-semantics`, `flui-animation`.
- **Render machine** — `flui-rendering` (the `RenderBox`/`RenderSliver` protocols),
  `flui-objects` (the concrete render-object catalog), `flui-engine` (layers → `wgpu`).
- **Spine & catalog** — `flui-view` (View/Element, `BuildContext`/`LifecycleContext`,
  reconciliation, signals), `flui-widgets`, `flui-runtime` (the frame runtime a realm drives,
  moving out of `flui-app` per ADR-0083; no host, platform or GPU edge), `flui-sdk` (the
  Evolving package-author surface, versioned `0.N` apart from the train; ADR-0088), `flui-testing`
  (a headless host that pumps a realm on a virtual clock, and the widget harness on it).
- **Official packages** (`packages/`, ADR-0088) — `flui-material`, `flui-cupertino` and
  `flui-devtools`, built on `flui-sdk` alone, as a third-party package would be;
  `flui-hot-reload` is an official package still under `crates/`; `flui-app` reaches it only
  through the `DevReloadHook` (ADR-0094 §1), and it moves once ADR-0088 settles its plugin
  half, which names `flui_view::__runtime` and the pipeline types the SDK does not carry.
- **Composition roots** — `flui-app` (per-window `UiRealm`s, the run loop), `flui-cli`, and
  the facade.

The non-obvious invariants live in the per-crate `ARCHITECTURE.md` files — read the one for the
crate you're changing before changing it.

## Working here

- **Isolate each task in its own worktree**; the shared checkout stays on `main`:
  `git worktree add -b <area>/<slug> ../flui-wt-<slug> origin/main`. Review someone else's PR
  from your own directory (`gh pr diff`/`checkout`), not inside their worktree.
- **Commits** `area: what changed`, one logical change each. **PRs** are one task each, with
  `cargo xtask check-changed` green first; CI is the proof. Before asking for review, review the
  branch against `main` yourself and list only what would block the merge: file and line, why it
  is wrong, how to show it fails. Risky PRs get the `full-ci` label (it runs the extended
  lane: every job, the nightly-only platform jobs included). Use
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

## Long runs

The maintainer usually hands over a whole task and comes back later.

- If a step doesn't need the maintainer's decision, keep going; put the status in the same
  message as the next action.
- Stop and ask only when you can't proceed without them, or before something irreversible or
  outward-facing: deleting data, force-pushing, merging, publishing, or changes outside your
  worktree.
- For a task of more than a few steps, keep a checklist in `TASKS.md` at the worktree root
  (git-ignored): tick what's done, append what you find. It survives context compaction and
  shows where the run is.
- Split large sweeps (an audit, a migration across many crates) between subagents with
  disjoint files; check each one's evidence before accepting it.
- End every run with three sections: **Waiting on you**, **Changed**, **Found** — with the
  command output behind each claim, and what you could not verify.

## Commands

| Need | Run |
|------|-----|
| Every task | `cargo xtask --help` (crate `tools/xtask`; the alias is in `.cargo/config.toml`). Anything else is a plain `cargo` command |
| Before a PR | `cargo xtask check-changed` — fmt + clippy + nextest over changed crates and their dependents (the classification CI's `plan` uses) |
| Full local gate | `cargo xtask ci` = `cargo xtask gate` (`checks`: fmt, typos, taplo, docs-links, docs-paths, workspace, reach, toolchain, wgsl, …; `lint`; `doc-strict`) + `cargo xtask test` + doctests |
| CI heavy jobs locally | `cargo xtask ci-full`; `cargo xtask doctor full` names any missing tool; job table in `docs/testing.md` |
| One crate / one test | `cargo nextest run -p <crate>`, `cargo nextest run -p <crate> <test> --no-capture` |
| Other targets (no link) | `cargo xtask cross-typecheck` — clippy for Win32 / AppKit / Android / iOS |
| Dependencies | `cargo xtask deps` — cargo-deny (bans, licenses, sources, advisories) over every member, and cargo-shear (`cargo shear --fix` applies its fixes) |
| Examples | `cargo run --example counter`, `cargo run --example <name>` (without a name, cargo lists them) |
| Render-object catalog | `cargo test -p flui-objects --test render_object_harness` |
| Toolchain | `rust-toolchain.toml` is the source of truth; pre-1.0 the MSRV tracks latest stable. `cargo xtask toolchain` keeps every copy in sync |

Gotchas: nextest doesn't run doctests (`cargo test --doc`). A flaky test that isn't yours usually
mutates a genuinely process-global resource (`Registry::global`) — scope a lock
to that test module rather than serializing the suite. The dev host is shared and
memory-limited: one compiling worker, a shared `CARGO_TARGET_DIR`; a docs-only change needs only
`cargo xtask checks`, which builds xtask and not the workspace.

## What the compiler and gates enforce

| Rule | Enforced by |
|------|-------------|
| Presentation capabilities (`rebuild_handle`, `writer_source`, `post_frame_handle`, `focus_manager`, `text_input_handle`, `keep_alive_*`, `pipeline_owner`, `async_driver`, …) are acquired only in `init_state`/`did_change_dependencies` | type system: they live on `LifecycleContext`, which only those hooks receive (ADR-0078) |
| Signals are read in `build`, never written or created there | run-time guard in `flui-view::reactive` (ADR-0074) |
| **ID offset** — slab indices are 0-based. Plain slab-backed IDs (`ViewId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`: insert `slab_index + 1`, look up `id.get() - 1`. Generational keys (`ElementId`, `RenderId`, `RealmId`) pack the 0-based slot and a non-zero generation: mint with `new_gen(slab_index, generation)`, read `.index()` | `NonZeroUsize` / `NonZeroU64` + ID newtypes |
| No lock guard held across an `if let`/`match` arm | `clippy::significant_drop_in_scrutinee` |
| No `todo!`/`unimplemented!`/`dbg!` in production (linux/ios/android init stubs carry an `#[expect]`) | clippy `todo`/`unimplemented`/`dbg_macro` |
| No `println!`/`eprintln!` in `flui-foundation`/`flui-macros` | clippy `print_stdout`/`print_stderr` |
| Logical and device geometry don't mix: no `Point + Point`, no `Size` as an `Offset`, no `DevicePoint` as a `Point`, no literal `DevicePixelRatio`, no `f64`/`i32` geometry mixing (ADR-0098) | trybuild suite `crates/flui-painting/tests/compile_fail/` |
| No bare `unwrap()` in production; by convention `expect("BUG: <invariant>")` for internal invariants, `thiserror` in libraries, `anyhow` in apps ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)) | `clippy::unwrap_used`; the conventions are review |
| Crate layering (a normal or build dependency points to a lower tier, or a smaller `order` in the same tier, unless the dependent lists it in `edge-exceptions` with the ADR that removes it; and, until `layer` is removed, to the same layer or lower — ADR-0081); no framework crate but `flui-app`, `flui-cli` and the facade links `flui-log`; none but `flui-app` depends on `flui-platform` (ADR-0082); only applications name an official package, in any dependency kind, an official package names another only through a declared exception, and an official package's normal and build dependencies are `flui-sdk` and the contract crates, each refused edge needing the dependent's `edge-exceptions` entry; a member under `packages/` is official and lists no exception (the kind rule, ADR-0081 §3, ADR-0088 §2); manifests inherit the workspace keys and lints, except that a `tier-kind = "evolving"` crate sets its own `0.N` version; `flui-foundation`, and no other member, declares the train guard `links = "flui_train"` (ADR-0088 §5); no unreachable test file; unique ADR numbers | `cargo xtask workspace` (`[package.metadata.flui]` in each manifest) |
| No crate reaches what its tier forbids (`[workspace.metadata.flui.reach]`, where H forbids nothing, plus its own `reach-forbid`) in any root build, over normal and build edges on every target, except through a `reach-exceptions` entry that names its ADR and still excuses something; hot reload stays out of `flui-app`'s graph under every feature (ADR-0081 §2, ADR-0094 §1) | `cargo xtask reach` |
| Import direction between a crate's top-level modules (flui-widgets): non-test code names only modules in lower layers, through re-exports too; `#[cfg(test)]` code is exempt; a refused edge needs a dated `exceptions` entry naming the ADR that removes it | `cargo xtask module-dag` (`[package.metadata.flui.modules]`) |
| No dependency that no code uses, no test-only dependency in `[dependencies]`, no `[workspace.dependencies]` entry nothing inherits (an optional dependency, or one a feature names, is only warned about); licenses, sources and banned crates per `deny.toml`, including crates std now replaces (`once_cell`, `cfg-if`, …); RustSec advisories | `cargo xtask deps` (cargo-shear, cargo-deny; CI's `deps` job) |
| No new `static` or `thread_local!` outside `#[cfg(test)]` without a `[package.metadata.flui] globals` entry: an `exit` ADR that removes it, or a `grant` under ADR-0097 with a checked `class` (one `trampoline` in the host, one per platform backend); an entry for a removed or exempt global is a finding | `cargo xtask globals` (ADR-0097), part of `cargo xtask checks` |
| Links from the non-archival markdown into the checkout resolve without climbing out of it: files, `#heading` anchors, and this repository's own `main` URLs | `cargo xtask docs-links` (lychee, offline), part of `cargo xtask checks` |
| A repository path in a code span of the non-archival markdown or `llms.txt` (a word starting at a top-level directory such as `crates/` or `docs/`), a link in `llms.txt` (an `#anchor` into Markdown included), and the package after `-p`/`--package`/`-p<name>` in a cargo command (a local package; a `Cargo.lock` one for `cargo update`/`tree`/`pkgid`/`clean`) name something git knows; changelogs are history and not read | `cargo xtask docs-paths`, part of `cargo xtask checks`; allowlist `tools/xtask/allowlists/docs-paths.toml`, exact counts that only shrink, a reason each |
| No process markers (`Cycle N`, `Phase B`, wave and slice labels, `PR-N`, spec task ids, `H`-tracker ids) in comments, doc comments, strings, the snake-case pieces of identifiers (`test_t064_x`), Markdown text or TOML/YAML/WGSL files read whole, outside the archival roots | `cargo xtask markers`; allowlist `tools/xtask/allowlists/markers.toml`, exact counts that only shrink |
| At most 3000 production lines per `.rs` file (test-only modules and items excluded) | `cargo xtask file-length`; allowlist `tools/xtask/allowlists/file-length.toml`, exact counts that only shrink |
| A `changelog.d/` fragment has a known section header, only bullets under it, and only root-relative or absolute links; `CHANGELOG.md` has one `## [Unreleased]` holding only those sections | `cargo xtask changelog --check`, part of `cargo xtask checks` |

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
few hundred tests): a new test needs a reason to exist next to the ones already there. No gate
caps the count, so the review question is which existing table the new case joins.

- **Test through the public API.** A test lives in `tests/` and sees what a consumer sees. An
  in-`src` `mod tests` is for what a consumer cannot reach: a failure-path matrix that needs a
  private seam, a decision recorded in `## Mapping decisions`. Compile-fail cases are not among them: they are
  trybuild fixtures driven from `tests/` (or `compile_fail` doctests on public items), so
  privacy and sealing are checked the way a consumer meets them. Do not pin private fields or
  helpers, which dirty flag a setter raises, `size_of`, an implementation's constants and token
  tables, or `Default`/`Debug`/getter round trips: a refactor that keeps behavior must not touch
  a test. Values a consumer sees and a document fixes (wire spellings such as `flui-protocol`'s
  ADR-0080 names, ABI, other ADR-pinned tokens) are contract, and their tests stay.
- **One behavior, one test; a family is one table.** Cases that differ only in their input are
  rows of one table-driven `#[test]`: each row a plain `fn` named after the case, every row run
  after an ordinary panic, and the failure report naming each failing row. Use the crate's
  existing runner (`table_test::run_table`, `test_cases::run_cases`, `tests/contracts.rs`)
  instead of a new one. Most runners do not contain a panic payload whose `Drop` itself panics
  (those in `flui-foundation` and `flui-animation` do): a row must not throw one. A new
  `#[test]` beside a near-identical one is a row.
- **Few binaries.** Every root `tests/*.rs` file is its own binary: it links the whole dependency
  stack and grows `target/`. Crates build their integration tests as modules of one binary
  (`tests/main.rs` with `#[path = "x.rs"] mod x;`, `autotests = false` and one `[[test]]` in the
  manifest, as `flui-widgets`, `flui-material` and `flui-rendering` do). A new file is a new
  `mod` line, not a new `[[test]]` (with `autotests = false` only the `[[test]]` entries are
  targets). Subdirectories are never auto-discovered as targets: a helper directory
  (`tests/common/`, `tests/support/`) is mounted from `main.rs` as a module, a trybuild fixture
  directory (`tests/ui/`) is not mounted at all, since its sources are meant not to compile. A
  separate target is for process-global state (`Registry::global`, a global subscriber,
  allocation counting) and for a feature the rest of the crate builds without.
- **Do not fold what runs its own process.** Tests that spawn `cargo` or another program
  (trybuild suites, `cli_create::generated_*`, `flui::facade_consumer`) stay separate tests:
  `.config/nextest.toml` names them one by one (groups `trybuild` and `nested-cargo`) so nextest runs them in
  parallel, and a folded one runs serially and holds the whole job. GPU readbacks share a
  single-threaded group and fold freely.
- **Keep what the Definition of Done requires.** Every concrete `RenderBox`/`RenderSliver` has
  a row in the `render_object_harness` family tables (`RENDER_OBJECT_TYPES` is checked against
  them); a decision in the crate's `## Mapping decisions` has its test, named there; a
  failure-path matrix keeps each failure point alone, two in competition, and the next
  operation after containment.
- **Test names are references.** ARCHITECTURE.md files, ADRs and `docs/` cite tests by name:
  `rg` the name before renaming, folding or deleting a test.

What only CI sees: the host compiles one platform, so a test written on Windows can be red on
Linux, macOS or wasm.

- A helper kept alive by `cfg(any(target_os = "windows", test))` is dead once the test that used
  it goes: gate the item, and any import only a `cfg`'d test uses, with the same `cfg`.
- Once a function stops being `#[test]` (a table row), clippy applies `unwrap_used` to it.
- A test that reads its own source with `include_str!` must not depend on line endings.
- `cargo clippy --all-targets --target x86_64-unknown-linux-gnu` and `--target aarch64-apple-darwin`
  check unix test code without linking (without `--all-targets` the test targets are skipped),
  except in crates whose dependencies have a C build script; `cargo xtask wasm-check` covers wasm.

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
| Dependencies, layering, a new crate | root `Cargo.toml` (`[workspace.metadata.flui] tiers` and `layers`, `[workspace.dependencies]`), `docs/crates.md` |
| Writing a frame-driving test | `docs/testing.md` (use the shallowest tier that can fail), `crates/flui-rendering/docs/TESTING.md` |
| Contracts, pipeline, panics | `docs/FOUNDATIONS.md`, `docs/architecture.md`, `docs/PANIC-POLICY.md` |
| Planning a large change, git hygiene | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

## Review guidelines

Pull requests are reviewed by Codex, which reads this section; a human reviewer can use it the
same way. fmt, clippy (pedantic, `unwrap_used`, the lints in the table above), rustdoc and the
script gates already run in CI, so style and anything they catch is not worth a comment.

- **What to report:** defects that would make a maintainer block the merge. Each finding names
  the defect and a concrete failure scenario — the input or sequence that produces the wrong
  result. If you can't construct one, label it a hypothesis. No praise, no restating the diff.
- **Tests:** for each behavior change, find the test that covers it and ask whether it would fail
  with the production hunk reverted. Tests here have passed both ways by reimplementing the
  predicate they pin, asserting that a widget exists rather than that it was laid out or
  painted, pinning a `Send` bound with a type that already satisfies it, counting rebuilds
  through a harness helper that dirties the root itself, or narrowing an assertion to what a
  partial implementation handles. A regenerated `*.snap` is a claim the new output is correct —
  the PR must say what changed and why. A test that mutates genuinely process-global state
  (`Registry::global`) needs a module-scoped lock, because nextest runs one
  process per test in parallel.
- **Failure paths and recovery:** do not stop at the first reported error or panic. Inventory
  every owned value, guard, callback and deferred obligation still live at each failure boundary,
  including user-defined generic values whose `Drop` can panic. Exercise each failure point
  alone, two failures in chronological competition, and the next operation after containment;
  the first failure must remain authoritative and the subsystem must still make progress. For
  queued or coalesced work, test durability and liveness separately: fail delivery, restore the
  hook, repeat the same id, cross independent handles sharing the state, and prove that a handle
  without a delivery hook cannot erase pending wake debt. See
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
  `--locked` on every cargo call, caches saved only on `main`, a job's name equals its key, and a
  new job is listed in the `ci` aggregator's `needs` (a lane-gated one also in `HEAVY_JOBS`,
  `FULL_JOBS` or `EXTENDED_JOBS`, matching its `if:`).
- **Registries and exemptions** (`RENDER_OBJECT_TYPES`, `docs/ROADMAP.md`, a `deny.toml` skip, a
  `typos.toml` word, an `#[expect]`): check that each entry matches the code in the same PR and
  that a new exemption states its reason.
- **Docs:** no process markers (see "Working here"); no hand-maintained completeness claims ("all
  call sites now use X") without the command that showed it; no claim of matching
  another framework without the reference it was checked against.
