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
- **Rules are types:** the compiler first, a clippy lint next, a grep last.
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
- workspace-wide changes that skip features, examples, shaders or tooling.

## Codebase map

`docs/crates.md` maps the crates, `packages/` and the facade (`src/`); each manifest's
`[package.metadata.flui]` gives tier and layer. Not visible from the layout:

- only `flui-app` depends on `flui-native`, and `windows::*`/`objc2::*` types stay in it
  (ADR-0082); `flui-runtime` has no host, platform or GPU edge (ADR-0083);
- official packages build on `flui-sdk` alone (ADR-0088); `flui-hot-reload` is one, still under
  `crates/`, reached only through `DevReloadHook`;
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
- A consumer-visible change adds `changelog.d/<branch-slug>.md`
  (`### Added|Changed|Deprecated|Removed|Fixed|Security` and bullets), so parallel PRs never
  edit `CHANGELOG.md`. A platform-support change updates `docs/platform-parity.md` (✅ only
  after a run on that platform) and `docs/gaps.md`.
- Commits are `area: what changed`. CI executes Linux and wasm32 only; native and GPU changes are
  run locally.
- No process markers (`Cycle N`, `PR #NNN review`, `Phase B`, slice labels) in code or docs.
  Archival roots are exempt (`docs/plans`).
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
| Presentation capabilities (`rebuild_handle`, `focus_manager`, `text_input_handle`, `pipeline_owner`, …) are acquired only in `init_state`/`did_change_dependencies` | `LifecycleContext`, which only those hooks receive (ADR-0078) |
| Signals are read in `build`, never written or created there | run-time guard in `flui-view::reactive` (ADR-0074) |
| **ID offset** — plain slab-backed IDs (`ViewId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize` (`slab_index + 1`); generational keys (`ElementId`, `RenderId`, `UiRuntimeId`) are minted with `new_gen(slab_index, generation)` and read with `.index()` | ID newtypes |
| Logical and device geometry don't mix (ADR-0098) | trybuild `crates/flui-painting/tests/compile_fail/` |
| No bare `unwrap()` in production: `expect("BUG: <invariant>")`, `thiserror` in libraries, `anyhow` in apps ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)) | clippy |
| Dependencies point down the tiers; an exception names the ADR that removes it | `xtask workspace`, `xtask reach` (ADR-0081) |
| A Stable crate's public API names upstream types only as ADR-0089 §2 allows | `xtask api-closure` (CI `doc` job, nightly rustdoc JSON) |
| Every `static`/`thread_local!` outside tests has a `globals` entry (an ADR that removes it, or an ADR-0097 grant) | `xtask globals` |
| Every OS entry into `flui-native` (an `extern` fn, an Objective-C or COM method, an AccessKit handler, a closure handed to the OS) is a constant or one call of `owner::panic_boundary::os_entry`, or of an adapter of its file that is (ADR-0151 §16) | `xtask panic-boundary` |
| Test support is named `testing`; a hand-written `pump`/`advance`/`tick` or a `Mock*`/`Fake*`/`Scripted*`/`Recording*`/`Probe*` type in test code is in a kit or counted in the allowlist (ADR-0243) | `xtask testing` |

## ADR Policy

ADRs (`docs/adr/`) record the cross-crate decisions the code follows, in the MADR shape
[`docs/adr/README.md`](docs/adr/README.md) gives: drivers, every option with its pros and cons,
the decision as a Rust shape, and how compliance is confirmed. A change of decision is a new ADR
with `Supersedes:`, and `Superseded-by:` on the old one; code that disagrees with an accepted ADR
is a defect in one of them.

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

## Where to read next

`docs/architecture.md` (the code as it is), `docs/adr/` (the decisions), `docs/testing.md` (test
tiers), `docs/platform-parity.md` and `docs/gaps.md`, [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Review guidelines

Pull request reviewers (Codex, Claude Code Review, a local `/code-review`) work from this
section; [`REVIEW.md`](REVIEW.md) sets severity and scope for Claude Code Review. Report defects
with a concrete failure scenario, and code an experienced Rust developer would be embarrassed
by, with the better shape. What this repository keeps getting wrong:

- a new `pub` item that no production path reaches, unless the PR names the follow-up;
- a regenerated `*.snap` the PR doesn't explain;
- the "Recurring defects" above, checked at each failure point alone, two in competition, and
  the next operation after containment ([matrix](https://github.com/vanyastaff/flui/blob/083b0f75da003030e908d107825e9f4db4e54437/docs/research/signal-unwind-contract.ru.md));
- `SliverGeometry { ..SliverGeometry::ZERO }` (drops `layout_extent`, `visible`), geometry from a
  stand-in value (ADR-0054), intrinsics, baselines or hit-testing left at defaults;
- a pixel claim whose readback samples don't distinguish fixed from broken; a `wgpu::Instance`
  created apart from its surface;
- state owned by the process instead of a UI runtime; native-platform changes without a shown
  run; event translation tested only synthetically;
- shapes transliterated from another framework (Dart-style names, setter/getter pairs);
- a `type` alias for values that are easy to confuse, where a newtype makes the mix-up a type
  error; a `new` returning `Result` or `Option` (that constructor is `try_new`);
- a generic-return call (`.into()`, `.parse()`, `collect()`) whose target type the reader has to
  infer: name it with a turbofish or a typed binding;
- `SAFETY:` comments that restate the operation instead of naming the invariant;
- workflow actions not pinned to a full SHA, cargo without `--locked`, caches saved off `main`;
- an exemption (`deny.toml` skip, `typos.toml` word, `#[expect]`) without its reason.
