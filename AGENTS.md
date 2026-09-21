# AGENTS.md

> Compact guide for AI agents working in the FLUI repository. Every line answers: "Would an agent likely miss this without help?"

---

## Prime Directive

Three rules, in priority order. They override convenience, never each other.

1. **We take inspiration from Flutter; we do not match it.** The three-tree model (View → Element → Render), lifecycle, and the layout/paint/hit-test protocol are Flutter's ideas, and where they are good we start from them — but nothing is inherited wholesale. *Structure, architecture, and code style* are designed for Rust as it is now (Arity system, `NonZeroUsize` IDs, Slab arenas, `Result`/`thiserror`, and the divergences the ADRs record — ADR-0008, ADR-0018/21/30/37). Where we follow a Flutter contract, name which one and prove it with a test; where we improve on it — more type-safe, faster, safer, more ergonomic — the improvement is what the test asserts. Anything we improve on gets its reasoning written down: an ADR for a protocol-level contract, a `## Mapping decisions` entry in the crate's `ARCHITECTURE.md` for a local one. What is never acceptable is losing a behavior by accident — dropping one is a decision, recorded in the same place. What this rule protects unconditionally is the *framework user's* mental model: declarative widget composition over a retained three-tree, keys, lifecycle. See [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md) Part I and [`docs/PORT.md`](docs/PORT.md) §Mapping rules. Flutter source is optional reading, not a build dependency: `.flutter/` and `.gpui/` (Zed) are gitignored local clones, and a checkout at whatever revision you have beats none — but state the revision you actually read rather than implying a check you did not run.
2. **Search the market before settling.** Before adopting a design — Flutter's or your own — check what the current ecosystem does (Compose, SwiftUI, and the Rust frameworks: egui, Iced, Xilem/Masonry, Bevy UI, GPUI, Dioxus, Slint) and what the current Rust toolchain and crates offer, and pick the best-known shape, citing where it comes from. This applies to functionality, architecture, *and* code style alike: an idiom that is stable in today's Rust replaces the older pattern it supersedes. Breaking changes are cheap today and ossify once consumers exist; do not defer a better shape to "later". Where Flutter has *no strong contract* — animation curves, velocity prediction, color interpolation, input smoothing — Flutter is not even the baseline: propose the market-best abstraction directly. **Sanctioned leapfrog zones (ADR-0027):** multi-window ownership, runtime/scheduling topology, concurrency architecture, and presentation architecture — Flutter's widget-tree semantics are a starting point there, not a constraint; a review must not reject `UiRealm`-model divergence (realm-scoped GlobalKey/focus, per-realm schedulers) as forbidden drift.
3. **Done means the behavior is verified and the reasoning is written down.** Before claiming completion, the change has a test that would fail without it, and any contract we chose over Flutter's is recorded (ADR / `## Mapping decisions`). "Better than Flutter" without that accounting is an unverified claim, exactly as "same as Flutter" would be. [Definition of Done](#definition-of-done-anti-cheating) is the checklist.

---

## Quick Start for AI Agents

**Read this first.** Then pick your entry point from [Documentation](#documentation) below.

- **Create a PR** — run `just ci` first and fix any failures before committing. A PR body may say `close(s)`/`fix(es)`/`resolve(s)` `#N` only when the merge is meant to close that issue: GitHub's linker ignores negation and surrounding prose ("PR4 closes #N" closed #N), so write `Refs #N` otherwise.

---

## Code Navigation

This repo declares exactly one MCP server in `.mcp.json` — **cratesio**, for crates.io package, version, and docs.rs lookups. It answers questions about *external* crates only; it knows nothing about this workspace. (`.codex/config.toml` also lists it for Codex compatibility; use `.mcp.json` as the authoritative project declaration.)

Individual developers may have extra servers configured at user scope (a code-graph server, a notes vault); those are personal setup, not a repo contract — never assume one is present, and never make a workflow here depend on it.

---

## Tech Stack

FLUI is a Flutter-inspired declarative UI framework for Rust. Pipeline in order: immutable `View`
configuration → mutable `Element` lifecycle → layout/paint `RenderObject` → retained `Layer` tree →
`flui-engine` compositor → `wgpu` GPU. Current phase, and what lands when: [`docs/ROADMAP.md`](docs/ROADMAP.md).

Versions and the dependency set live in the root `Cargo.toml` (`[workspace.dependencies]`) — read
them there. What the manifest can't tell you:

- **Layering:** crates form a DAG, foundation → core → rendering → framework → app. Dependencies
  point one way down that DAG; see [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md)
- **Platform:** native Win32, AppKit, and headless backends, with `winit` only as a fallback
- **Diagnostics:** *emitting* is universal; *installing a subscriber* is a composition-root decision that lives in `flui-log` and never in a library (CI enforces the `tracing`-only rule in foundation/tree/macros crates via port-check trigger #15)
- **Errors:** `thiserror` (libraries), `anyhow` (applications); panics only per [`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md) — `expect("BUG: <invariant>")` for internal invariants, never bare `unwrap()` on production paths (`clippy::unwrap_used` gates this)

## Build & Development Commands

Two invocations `just --list` won't teach you:

```bash
cargo test -p flui-objects --test render_object_harness  # catalog guard for render objects
just port-check-verbose                                  # per-trigger pass/fail + marker totals
```

Both of CI's non-cargo gates run inside `just gate` (and so inside `just ci`), through the
`text-check` recipe — **`typos`** (config: `typos.toml`) and **`taplo fmt --check`** (config:
`.taplo.toml`). Each is skipped with a printed message when its binary is absent, so a green
`just ci` on a machine without them is weaker than CI's: install both
(`cargo install typos-cli taplo-cli`) if you want the local gate to mean what CI means.

## Architecture Constraints (port methodology)

These are enforced by `scripts/port-check.sh` in CI and locally via `just port-check`. Violating them will fail CI. See [`docs/PORT.md`](docs/PORT.md) for the full list of 23 refusal triggers plus FR-033.

| Rule | Why |
|------|-----|
| **ID offset pattern** — slab indices are 0-based; public IDs (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`. Insert: `slab_index + 1`; lookup: `id.get() - 1`. | Consistent across all crates |
| **No `RwLock<Box<dyn RenderObject>>`** in render/view/layer/painting/engine storage | Lock-or-interior-mutability problem |
| **No `async fn` in build/layout/paint/composite/render hot paths** | Sync pipeline per Flutter contract |
| **No `unimplemented!()`/`todo!()` in production code** (except platform-init stubs on linux/ios/android) | Triggers #8 |
| **No `Box<dyn View>` as struct fields** in element child collections | Recursive-box storage rejected |
| **No `From<f32>` for unit wrappers** in flui-geometry | Unit-barrier escape hatch guard |
| **Sanctioned `dyn` boundaries only** — see the allowlist in port-check.sh trigger #9 | FR-036 registry |
| **No locks in public API** (`pub fn -> MutexGuard`, `pub field: Mutex<...>`) | SP-6: locks behind private fields |
| **No dependency on `flui-log` outside `flui-app`, `flui-cli`, and the facade** | Libraries emit through `tracing` and must not reach the backend; `just inventory-check` enforces the `allowed_dependents` list in `docs/workspace-layers.toml` |
| **No `println!`/`eprintln!`/`dbg!`** in foundation/tree/macros crates | Use `tracing` macros |
| **No lifecycle-only presentation capability inside `build`/`perform_layout`/`paint`** — `rebuild_handle()` (ADR-0018), `post_frame_handle()` (ADR-0021), `text_input_handle()` (ADR-0030), and `focus_manager()` (ADR-0037) are acquired in `ViewState::init_state` / `did_change_dependencies` and used later | Trigger #22: mutation or scheduling from a frame phase can create an unbounded rebuild loop, re-enter the frame transaction, or leak ownership across presentations. Adding a capability to `BuildContext` means adding its token to `scripts/check-frame-capability-scope.sh` in the same change |

## Documentation

Entry points by task, then the reference documents with no task of their own.

| Need | Read | Then / notes |
|------|------|-------------|
| Add a new feature | `docs/ROADMAP.md` (is it planned?) | `docs/FOUNDATIONS.md` |
| Change render/layout/paint | `docs/PORT.md` (translation rules, refusal triggers, type map) | `.flutter/` reference |
| Understand error handling | `thiserror` in libs, `anyhow` in bins | |
| Touch logging setup or a log backend | Subscriber policies, native sinks, who may depend on the backend | `docs/workspace-layers.toml` (only composition roots may depend on it) |
| Write or review Rust code | `rustfmt.toml`, `clippy.toml`, the workspace lints in `Cargo.toml`, `docs/adr/` | Relevant architecture contract |
| Add a cross-crate dep | `docs/workspace-layers.toml` (the checked layer policy) | Root `Cargo.toml` `[workspace.dependencies]`, `docs/FOUNDATIONS.md` Part IV |
| Add a new crate | `docs/workspace-layers.toml` — classify it *first*; `[[planned]]` records gated extractions | `docs/crates.md` "Adding a New Crate", [ADR-0041](docs/adr/ADR-0041-workspace-topology-contract.md) |
| Catch up on recent changes | `CHANGELOG.md` | `docs/ROADMAP.md` |
| Understand GPU rendering | `crates/flui-engine/ARCHITECTURE.md` | Compositor, glyph atlas, pipeline topology |
| Write a test that drives a frame, or add test support | `docs/testing.md` — the map of the tiers; pick the shallowest one that can fail | `crates/flui-rendering/docs/TESTING.md` (RenderTester API, catalog rules) |
| **Foundations** | `docs/FOUNDATIONS.md` | Architecture contract, locked contracts (C1–C9) |
| **Architecture** | `docs/architecture.md` | Three-tree pipeline overview |
| **Panic policy** | `docs/PANIC-POLICY.md` | When `expect("BUG: …")` is allowed vs. `Result`; `clippy::unwrap_used` gate |
| **Runtime contract registry** | `docs/runtime-contract.toml` | Public shipped/planned runtime contracts, classified boundary families, and the checked root-export manifest. It deliberately does not depend on internal design records. Checked by `just runtime-conformance-check`; touching a monitored runtime export means updating it deliberately |
| **Crate ARCHITECTURE.md** | `crates/flui-{engine,foundation,layer,painting,platform,rendering,scheduler,widgets}/ARCHITECTURE.md` | Per-crate deep architecture |

## AI Context Files

`AGENTS.md` (this file) is the single agent guide, shared by every agent runtime. There are no
per-runtime shims and no separate path-scoped rule files: **put the substance here and nowhere
else**, or the runtimes drift apart. The Rust engineering standards that `STYLE.md` used to carry
now live in `rustfmt.toml`, `clippy.toml`, the workspace lints in the root `Cargo.toml`, and
`docs/adr/`, with the architecture-level rules in [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md).

## Error Triage

When you hit a build/test error:

1. **Port-check violation** → check `docs/PORT.md` for the trigger ID. The pattern you introduced is banned by the architecture contract.
2. **Render-object harness failure** → every exported `RenderBox`/`RenderSliver` must appear in `RENDER_OBJECT_TYPES` with a matching `harness_*` test. See `crates/flui-rendering/docs/TESTING.md`.
3. **Test flake** → the singleton family is retired, so a flake means a test is mutating a *genuinely* process-global resource (`Registry::global`, `FONT_SYSTEM` — named in `docs/runtime-contract.toml`'s ambient-reach ratchet), not a realm or scheduler. Add a lock scoped to that test module. The deleted test locks are no longer worth searching for.
4. **Type mismatch across crate boundary** → check if you're using the wrong ID type (1-based vs 0-based). See ID offset pattern above.

Anything else — a clippy warning, `todo!()` on a production path, a banned pattern — is the architecture table above or the standards in `rustfmt.toml`, `clippy.toml`, the workspace lints, and `docs/adr/`.

## Definition of Done (anti-cheating)

An agent reporting "done" makes a claim that later work is built on. A green gate is **necessary but not sufficient** — gates can be satisfied without implementing the behavior. The recurring failure mode in this repo is **"MVP reported as done"**: a change passes the harness and port-check but silently loses a behavior nobody looked at, on a path no test covers. Its mirror image — **"MVP reported as an improvement"** — is calling an accidental divergence "better" after the fact, with no ADR/mapping entry and no test asserting the new behavior; it is the same claim with a different label.

**Before reporting a render/layout/paint/lifecycle change done:**

1. **The behavior is verified, and a chosen divergence is recorded.** Every case is either matched or *deliberately* different, with the difference recorded (ADR / `## Mapping decisions`) and covered by a test that asserts the contract we actually ship. An audit finding with no cross-check is a hypothesis, not a fact; a divergence with no record and no test is a regression until proven otherwise.
2. **Every concrete `RenderBox`/`RenderSliver` carries harness tests** (catalog CI guard), and new behavior needs a test that would *fail* without the change.
3. **Scope is reported honestly.** "X done" from a prior session is not done — re-verify. State what is implemented vs deferred and *why*; never imply completeness you did not check. If a behavior is not implemented, say so explicitly rather than satisfying a gate with a stub, a narrowed test, or a default-returning intrinsic.

## Agent Rules

- **No internal process-ID markers in code** — this bans embedding internal process-tracking markers such as `Cycle N`, `PR #NNN review`, or `Phase B`. Repo-specific are two families named nowhere else — bare `U##` step-citations and spec `SC-NNN` success-criteria numbers — plus the exception and the sweep's denominator. A marker is acceptable only when its meaning is defined beside its use (a test-case ID in the same file's legend) or is mechanically load-bearing (`FR-NNN`/`ADR-NNNN` references a checker greps). Sweeps may exclude only archival roots — `docs/{audits,brainstorms,ideation,plans,research,superpowers}`, `.rust-studio/specs`, `specs`, `openspec`; shipped docs such as crate `ARCHITECTURE.md` and `docs/ROADMAP-TRACKER.md` stay in scope. This defines the denominator, not a claim that every in-scope hit is gone; known residue is tracked in issue #644.
