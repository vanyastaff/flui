# ADR-0078: Rules live in types and lints; `LifecycleContext` carries the capabilities

- **Status:** Accepted
- **Date:** 2026-09-23
- **Amended:** 2026-09-26 — scanners over structured data return under four conditions; see
  §4 ([ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md)); an allowlist entry may also
  name the ADR that grants it for good ([ADR-0097](ADR-0097-no-process-global-state-gate.md)).
- **Supersedes:** the capability-acquisition clauses of ADR-0018, ADR-0021, ADR-0030 and
  ADR-0037 (the rule stays, its enforcement moves into the type system); the port methodology
  (`docs/PORT.md`) and its grep gates (`scripts/port-check.sh`,
  `scripts/check-frame-capability-scope.sh`, `scripts/check-signal-write-scope.sh`).

## Context

FLUI's architectural rules were enforced by about two thousand lines of bash: 24 numbered
"refusal triggers" and a dozen named guards, each a regular expression over `crates/`, plus
two brace-depth scanners for "capability acquired inside `build`" and "signal written inside
`build`". Every false positive was silenced with a `// PORT-CHECK-OK-*` marker; there were
396 of them, 250 for a single lock rule.

The scanners could only see text. `rebuild_handle()` inside `build` was caught, but the same
call behind a helper function (`fn subscribe(ctx: &dyn BuildContext)`) was not; a capability
stored under its own method name was flagged even when legal. The markers turned every
lock, every `dyn`, every public type into a small ritual, and the rules themselves were
spread over a 1300-line methodology document that mostly mapped Dart idioms to Rust.

Most of those rules can be stated where Rust checks them for free.

## Decision

### 1. Presentation capabilities are a type

`flui-view` splits the build context in two:

```rust
pub trait BuildContext: Sealed { /* identity, inherited lookups, ancestor walks, signal reads */ }

pub trait LifecycleContext: BuildContext {
    fn rebuild_handle(&self) -> RebuildHandle;
    fn async_driver(&self) -> Option<AsyncDriver>;
    fn post_frame_handle(&self) -> Option<PostFrameHandle>;
    fn local_post_frame_handle(&self) -> Option<LocalPostFrameHandle>;
    fn text_input_handle(&self) -> Option<TextInputHandle>;
    fn hit_test_handle(&self) -> Option<HitTestHandle>;
    fn keep_alive_lease(&self) -> KeepAliveLease;
    fn keep_alive_handle(&self) -> KeepAliveHandle;
    fn lifecycle_handle(&self) -> Option<LifecycleHandle>;
    fn focus_manager(&self) -> Rc<FocusManager>;
    fn pipeline_owner(&self) -> Option<PipelineCell>;
}
```

`ViewState::init_state` and `ViewState::did_change_dependencies` receive
`&dyn LifecycleContext`; `build` receives `&dyn BuildContext`. The runtime passes the same
object to both — only the static type differs — so there is no runtime cost and no second
code path. A capability acquired in `build`, directly or through any helper, is a compile
error:

```text
error[E0599]: no method named `rebuild_handle` found for reference `&dyn BuildContext`
```

A helper that acquires a capability on behalf of a hook takes `&dyn LifecycleContext`, which
documents the contract in its signature. Migrating the workspace touched five such helpers;
no call site acquired a capability inside `build`.

`LifecycleContext` is sealed through its `BuildContext` supertrait. A new capability is a
method on `LifecycleContext`, never on `BuildContext`.

### 2. Other rules move to the compiler, clippy, or a test

| Rule | Now |
|---|---|
| Signal written or created during `build` | the run-time guard (`SignalError::WrittenDuringBuild` / `CreatedDuringBuild`, ADR-0074) was already authoritative; the advisory scanner is gone |
| A lock guard alive through an `if let`/`match` arm | `clippy::significant_drop_in_scrutinee`, workspace-wide; the 33 existing sites bind the value with `let` first |
| `todo!`/`unimplemented!`/`dbg!` in production | clippy `todo`/`unimplemented`/`dbg_macro` (already on) |
| Printing from the foundation crates | clippy `print_stdout`/`print_stderr` in `flui-foundation`, `flui-tree`, `flui-macros` |
| `From<f32>` on a unit wrapper | `compile_fail` doctests in `flui-geometry` (already present) |
| `async fn` on the frame path | the trait signatures are synchronous; an `async` impl does not match them |
| Two ADRs sharing a number; a normal or build edge against the tier order (ADR-0081) | `cargo xtask workspace` |

### 3. The rest becomes design guidance, not a gate

Locks on per-node state inside `perform_layout`/`paint`, locks in public signatures, `dyn`
at unsanctioned boundaries, duplicate type definitions across crates, speculative `pub mod`,
and module-confinement rules (`glam` in the GPU backend, `lyon` in the tessellator) are
stated in `AGENTS.md` and the owning crate's `ARCHITECTURE.md`, and checked in review. They
were never reliably checkable by a regular expression, and a false sense of coverage was
worse than none.

### 4. When a gate may be a scan

The same move removed more than the port scanners. `cf46dfe20` (#1283) deleted
`docs/runtime-contract.toml` with `scripts/check-runtime-conformance.sh`,
`docs/panic-policy-allowlist.txt` with `scripts/check-panic-policy.sh`, the advisory
`publish-dry-run` CI job with `scripts/publish-order.sh`, and
`scripts/check-workspace-inventory.sh`, whose manifest checks became `cargo xtask workspace`.
They read text, their allowlists were hand-kept files beside the code, and the rules they
guarded were either types and clippy lints by then or not worth a gate.

New scanners return anyway, starting with the tier gate of ADR-0081. A scan is admissible
only when all four hold:

1. **No type or stock lint can state the rule.** A type or a clippy lint is still the first
   choice (§1, §2).
2. **It reads structured data** — `cargo metadata`, a `syn` AST, a TOML table — never a
   regular expression over source text.
3. **It has a `--self-test`** that runs it over a planted violation and fails unless exactly
   the planted findings come back, as `wgsl --self-test` and `workspace --self-test` do, and
   `cargo xtask checks` runs the self-test beside the scan.
4. **Its allowlist is data, not markers.** It lives in the manifests or a data file, is seeded
   by the scan's own first run, names the ADR whose change removes each entry, or the ADR that
   grants it for good, and only shrinks: an entry the scan no longer needs is a finding. A
   permanent grant names its class, which the scan checks as far as the source allows
   (ADR-0097's `grant`/`class`). No inline comment silences it; inline markers are how the
   `PORT-CHECK-OK-*` sites reached 396.

The tier gate meets the first condition because nothing else sees the package graph. Types and
clippy work inside one crate. Cargo rejects only dependency cycles, not direction.
`disallowed_types` and `disallowed_methods` cannot forbid a dependency. cargo-deny's bans are
global or per dependent crate and cannot express a partial order (ADR-0041). A custom lint was
rejected below for its nightly toolchain.

The panic allowlist does not return: `clippy::unwrap_used` states that rule. The publish dry
run returns only as the `package-check` and `release-check` commands, under these four
conditions.

## Consequences

- `ViewState` implementations change one type in two signatures
  (`&dyn BuildContext` → `&dyn LifecycleContext` in `init_state` and
  `did_change_dependencies`). Code that only reads inherited data in those hooks is
  otherwise unchanged, since `LifecycleContext: BuildContext`.
- The statement-level lock-drop shape (`*slot.lock() = value;` displacing a value with a
  significant `Drop`) is no longer checked mechanically; clippy covers the scrutinee shapes,
  which is where the recorded deadlocks came from. `significant_drop_tightening` was measured
  (130 hits, mostly false positives) and not adopted.
- The runtime contract registry dropped its `lock_exemption` table, which existed only to
  mirror the `PORT-CHECK-OK-SP6` markers; the registry itself has since been removed.
- CI's `checks` job loses the port-check step and its `ripgrep` install.

## Alternatives rejected

- **A run-time phase assertion in each capability method.** `init_state` runs through the
  same `BuildCtx` as `build`, so the context cannot tell the phases apart without a new
  flag, and a debug-only panic finds the bug later than the compiler does.
- **Keep the scanners, drop only the markers.** The markers existed because the scanners
  could not tell legal from illegal code; without them every lock and `dyn` would fail.
- **A custom lint (dylint).** Real precision, but a nightly-pinned toolchain and a
  maintained lint crate for rules that types and stock clippy already cover.
