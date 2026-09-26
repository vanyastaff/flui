# ADR-0097: Process-global state is gated: one trampoline cell, everything else realm-owned

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends (on acceptance):** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (its open question "the runner's
  thread-local `AppRuntime` slot is the sanctioned transitional form" becomes a named,
  permanent exception class: OS-callback trampolines reach exactly one host cell),
  [ADR-0047](ADR-0047-unified-execution-services.md) (execution services stay loop-scoped; any
  process-global executor state the scan finds is an allowlist entry that can only shrink)
- **Related:** [ADR-0016](ADR-0016-unified-font-system-registration.md),
  [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §6,
  [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md),
  [ADR-0050](ADR-0050-global-key-identity-and-frame-reservations.md),
  [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md),
  [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md),
  [ADR-0092](ADR-0092-per-realm-text-over-parley.md),
  [ADR-0093](ADR-0093-router-is-the-primary-navigation-api.md),
  [ADR-0094](ADR-0094-hot-reload-through-subsecond.md)
- **Refs:** principle P3 and its gate in the [decision index](../../design/decisions.md); the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md)

Nothing in `crates/` or `tools/` changes as part of this ADR.

## Context

ADR-0027 makes the realm the owner of UI state: scheduler, focus, GlobalKeys, tickers. The code
still carries process-global and thread-global state that no realm owns. Examples, each
re-checked:

| Item | Where | Kind |
|---|---|---|
| `FONT_SYSTEM` | `crates/flui-painting/src/text_layout/layout.rs:124` | `static OnceLock<Arc<Mutex<FontState>>>` |
| `TIME_DILATION` | `crates/flui-scheduler/src/config.rs:43`, written at `:94` | `static AtomicU64` holding configuration |
| `AssetRegistry::global` | `crates/flui-assets/src/registry/mod.rs:83-90` | `static LazyLock<AssetRegistry>` behind a `pub fn` |
| `APP_RUNTIME` | `crates/flui-app/src/app/runner/host.rs:25-47` | `thread_local!` host of every realm |
| `POLLING_PENDING_WINDOWS`, `PENDING_SECONDARY_WINDOW_OPENS`, `PENDING_SECONDARY_WINDOW_COMPLETIONS` | `crates/flui-app/src/app/runner/secondary_window.rs:453-471` | `thread_local!` queues beside the host cell |
| `REQUEST_REBUILD` | `crates/flui-hot-reload/src/dispatch.rs:24` | `static LazyLock<Mutex<…>>` hook slot |
| `REGISTRY_STACK`, `TEST_REGISTRY` | `crates/flui-view/src/key/registry.rs:198-212` | `thread_local!`, `ManuallyDrop` for a hot-reload cdylib |
| `NAVIGATOR_COMMAND_TARGETS` | `crates/flui-widgets/src/navigator/navigator.rs:90-93` | `thread_local!` routing table |
| `LOCAL_LANES`, `ACTIVE_LANES` | `crates/flui-interaction/src/routing/interaction_lane.rs:739-741` | `thread_local!` lane registries |

Beside them sit monotonic ID counters (`KEY_COUNTER`, `crates/flui-foundation/src/key.rs:59`;
`NEXT_GRAPH_ID`, `crates/flui-view/src/reactive/mod.rs:72`; `NEXT_INCARNATION`,
`crates/flui-app/src/app/runtime.rs:190`, and others), warn-once flags
(`crates/flui-engine/src/batches/gradients.rs:64,77`), and platform cells that OS callbacks read
to find their owner (`DELEGATE_STATE`, `crates/flui-platform/src/platforms/ios/platform.rs:275`;
`ACTIVE_EVENT_LOOP`, `crates/flui-platform/src/platforms/winit/platform.rs:772`;
`ON_OWNER_QUEUE`, `crates/flui-platform/src/platforms/macos/owner_lane.rs:61`). This list is
**not** the inventory: a regex count over `crates/` and `src/` (lines matching an upper-case
`static` item, and `thread_local!` blocks, outside `*_tests.rs` and `tests/`, crate examples included) finds about 91 and
29, and it cannot tell a counter from configuration or leave out inline test modules. The inventory
is whatever the scan below produces.

This state costs three things. It breaks isolation: two realms share one font system and one
time dilation, and `TIME_DILATION` contradicts ADR-0027 §8 ("Tickers and animation controllers
belong to their realm's scheduler"). It breaks code patching: a copy of a static in a second
image, or a reset thread-local, splits whatever lives there — the dlopen worker already needed
a fix for its own `FONT_SYSTEM` (`docs/hot-reload.md:213`), and ADR-0094 makes globals reduction
a precondition of its spike. It breaks determinism of tests that run in one process.

**Nothing guards against a new one.** `docs/runtime-contract.toml` and its checker were
deleted deliberately in cf46dfe20 (#1283), whose message says those rules "are types and
clippy lints now (ADR-0078)". Its singleton net, though, only matched two idioms,
`impl_binding_singleton!(` and `fn instance() -> &'static`, and said a singleton built another
way "is invisible to this net" (`git show cf46dfe20^:docs/runtime-contract.toml`, lines 26-30).
None of the items above would have tripped it. No type and no stock clippy lint forbids an
interior-mutable `static`. `cargo xtask checks` runs seven in-process checks and none of them
looks at statics (`tools/xtask/src/tasks/checks.rs:100-123`).

## Decision

### 1. `cargo xtask globals` scans every static and thread-local

A new command parses the Rust source of every workspace member's library and binary targets
with `syn` and reports each `static` item (module-level or inside a function body, `static mut`
included) and each item inside a `thread_local!` invocation. It skips items under `#[cfg(test)]`,
and the `tests/`, `benches/` and `examples/` directories, as the other workspace gates exempt
examples and tools. `Atomic*` statics are findings like any other: `TIME_DILATION` is
configuration, not an identifier.

### 2. Two shapes are exempt; one class is a named exception; everything else is listed

- **Immutable data** is exempt: a `static` whose type is a primitive, a `&'static` reference to
  immutable data, or an array or slice of those. Anything with a lazy cell, a lock, a `Cell`,
  a `RefCell` or an atomic is not immutable data.
- **Monotonic ID counters** are exempt: an `Atomic{U,I}{32,64,size}` static whose every use in
  its file is `fetch_add`. A counter that is ever `store`d or `load`ed is configuration and is
  a finding.
- **OS-callback trampolines** are the one named exception class: a thread-local that a function
  invoked by the OS without a user-data pointer (a Win32 window procedure, an AppKit or UIKit
  delegate method, a JNI entry point, winit's event-loop borrow) reads to find its owner. The
  host has **exactly one**: `APP_RUNTIME`. Each backend module under
  `crates/flui-platform/src/platforms/` may hold at most one of its own. The entry is declared
  with class `trampoline`, and the gate fails if a crate other than `flui-app` and
  `flui-platform`, or a second cell in the host or in one backend module, claims it. The secondary-window queues
  beside `APP_RUNTIME` are not trampolines; they fold into it or into a realm.
- **Everything else** is an allowlist entry with a reason and an exit: the ADR or issue whose
  change removes it (`FONT_SYSTEM` → ADR-0092; `NAVIGATOR_COMMAND_TARGETS` → ADR-0093;
  `REQUEST_REBUILD` and `REGISTRY_STACK`'s `ManuallyDrop` form → ADR-0094; `TIME_DILATION`
  → a property of each presentation's frame clock).

### 3. The allowlist lives in the manifests, is seeded by the scan, and only shrinks

- Entries go in each crate's `[package.metadata.flui]` as `globals`, following #1283's move of
  rules into the manifests; `globals` joins the keys `cargo xtask workspace` accepts
  (`tools/xtask/src/workspace.rs:76-82`).
- The PR that adds the gate seeds the allowlist from the scan's own output, in the same PR —
  not from this ADR's table, and not from regex counts.
- The gate fails on a finding with no entry (a new global) **and** on an entry with no finding
  (a removed global whose entry was left behind), so the list can only shrink.

### 4. It reaches the merge path and can fail

`globals --self-test` plants an interior-mutable `static`, a `thread_local!`, an atomic that is
`store`d, a second trampoline cell, a pure `fetch_add` counter and a stale entry in a scratch
crate, and asserts the first four and the stale entry are reported and the counter is not. Both
`globals --self-test` and `globals` join the in-process list of `cargo xtask checks`, and the
test that pins that list (`checks.rs:149-172`) gains them.

### Why a syn scan and not a type or a lint

ADR-0078 moved rules into types and clippy and rejected regex scanners and dylint. There is no
type that makes an interior-mutable `static` unwritable, and stock clippy has no lint for one.
`clippy::disallowed_macros` can flag `thread_local!` but not `static`, and its per-site
`#[expect]` is local: a new exemption never passes through one reviewed list. A `syn` parse is
not a text match — a static behind a helper function or a type alias is still an item — and it
runs on the pinned stable toolchain, which dylint does not.

**Limits the gate does not claim to cover:** statics produced by macro expansion (the scan sees
source, not expansions; `once_cell` is already refused in workspace manifests by `deny.toml`'s std-replacements check, `deny.toml:120-128`), state
held in dependencies, and state selected through environment variables such as `FLUI_HEADLESS`.

## Alternatives considered

- **Restore `runtime-contract.toml`.** Rejected: its net saw two idioms and would have missed
  every global listed in Context; the file was a 4,085-line registry of text citations.
- **A regex over `static` and `thread_local!`.** Rejected for the reasons ADR-0078 gives, and
  because it cannot tell a counter from configuration.
- **Allow every `Atomic*` static.** Rejected: `TIME_DILATION` is an atomic and is exactly the
  kind of process-wide configuration P3 forbids.
- **Forbid every thread-local, trampolines included.** Rejected: an OS callback with no user-data
  pointer has no other way to reach its owner. The exception is kept narrow and named instead.
- **One central allowlist file.** Considered; the manifests already carry `allowed-dependents`
  and `wasm`, and one more key keeps each crate's exemptions next to the crate.

## Consequences

- Adding a process-global becomes a manifest change with a reason and an exit, visible in
  review.
- ADR-0027's transitional host slot becomes permanent by name, and nothing else may join it in
  the host.
- ADR-0016 legitimizes `FONT_SYSTEM` until ADR-0092 lands; it is allowlisted with that exit.
- Removing each entry is its own change with its own test; this ADR only freezes the count.
- `TIME_DILATION`'s public setter moves to the presentation's clock, a breaking change for
  callers of the process-wide setter.
- A static that is only a debug-counter under `#[cfg(test)]` stays invisible to the gate by
  design.

## Verification

None of this exists yet: `tools/xtask/src/main.rs` has no `globals` command and
`tools/xtask/Cargo.toml` does not depend on `syn`.

- `cargo xtask globals --self-test` fails on each planted violation and on the stale entry, and
  passes the counter (§4).
- `cargo xtask globals` is green on the seeded allowlist and is part of `cargo xtask checks`,
  pinned by the checks-list test.
- Each removal lands with a test that fails with the global back: two realms with different time
  dilation animate at different rates; two realms register different fonts and each shapes only
  with its own; a navigation intent reaches the realm that owns the router.
