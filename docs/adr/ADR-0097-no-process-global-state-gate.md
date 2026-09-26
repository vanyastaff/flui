# ADR-0097: Process-global state is gated: one trampoline cell, everything else realm-owned

- **Status:** Accepted in part (2026-09-26): §1–§4, the gate (`cargo xtask globals`) and its
  seeded allowlist; removing each global remains with its exit ADR. For the entries whose exit
  is this ADR (`TIME_DILATION`, the asset `REGISTRY` and `INTERNER`, `ERROR_VIEW_BUILDER`, the
  decoded-image `CACHE`), that removal is the part of this ADR still Proposed.
- **Date:** 2026-09-25
- **Amends:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (its open question "the runner's
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

The gate is `tools/xtask/src/globals.rs`; the entries live in the crate manifests. No global is
removed by this ADR.

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

A new command parses the Rust source of the library, proc-macro and bin targets of every crate
under `crates/` and the facade with `syn`. Applications (`tier-kind = "tool"`: the examples,
`tools/` and `flui-cli`, whose state belongs to its own process) are not scanned, as the other
workspace gates exempt them; nor are test, bench, example and build-script targets. The walk
starts at each target's root and follows `mod x;` as rustc does (`x.rs` or `x/mod.rs`, the
non-mod-rs directory rule, inline modules, `#[path]`); a declaration that resolves to no file,
or a file syn cannot parse, fails the gate.

A global is each `static` item at any depth (modules, fn, impl and trait-method bodies, blocks,
closures, `const` blocks; `static mut` included), each entry of a `thread_local!`, and each
`static` in the tokens of any other macro invocation or `macro_rules!` body, which catches the
statics FLUI's own macros emit into their callers (`app_plugin!`). cfg is evaluated three-valued
with only `test` known false, so an item is skipped only when an enclosing cfg is false whatever
the build (`#[cfg(test)]`, `all(test, …)`); `any(test, feature = "…")`, `debug_assertions` and
every platform cfg are scanned, and the result does not depend on the host. `Atomic*` statics are
findings like any other: `TIME_DILATION` is configuration, not an identifier.

### 2. Two shapes are exempt; one class is a named exception; everything else is listed

- **Immutable data** is exempt: a non-`mut` `static` (never a thread-local) whose type is a
  primitive, a shared reference to `str` or to immutable data, an array, slice or tuple of
  immutable data, or a bare `fn` pointer. Anything with a lazy cell, a lock, a `Cell`, a
  `RefCell` or an atomic is not immutable data; nor is a type alias or a user type, which the
  scan cannot see through.
- **Monotonic ID counters** are exempt: a non-`mut`, non-`pub` (private or `pub(…)`) static of
  an atomic integer type, initialized with `<type>::new(<integer literal>)`, whose every mention
  where it is visible (its fn body, its module's files and their descendants, or every file of
  the target for `pub(…)`) is the declaration, `NAME.fetch_add(<integer literal ≥ 1>, …)` or a
  `use` path segment without `as`. A counter that is `load`ed, `store`d, borrowed (`&NAME`),
  named in a macro, or advanced any other way is a finding. The exemption is about isolation,
  not determinism: an ID that reaches a snapshot is still process-ordered.
- **The key** of a global is its item path inside the target, from the crate root without
  `crate::`: fn, impl self-type and trait names are segments, a macro is `name!`, and a bin
  target's items are prefixed with the bin's name (`text_layout::layout::FONT_SYSTEM`,
  `registry::AssetRegistry::global::REGISTRY`, `plugin::app_plugin!::__FLUI_APP_STATE`). Line
  numbers are not part of it; definitions sharing a key under different cfgs are one global.
- **OS-callback trampolines** are the one named exception class: a thread-local that a function
  invoked by the OS without a user-data pointer (a Win32 window procedure, an AppKit or UIKit
  delegate method, a JNI entry point, winit's event-loop borrow) reads to find its owner. The
  host has **exactly one**: `APP_RUNTIME`. Each backend module under
  `crates/flui-platform/src/platforms/` may hold at most one of its own. The entry is declared
  with class `trampoline`, and the gate fails if a crate other than `flui-app` and
  `flui-platform`, or a second cell in the host or in one backend module, claims it. The secondary-window queues
  beside `APP_RUNTIME` are not trampolines; they fold into it or into a realm.
- **Everything else** is an allowlist entry with a reason and either an `exit`, the ADR whose
  change removes it (`FONT_SYSTEM` → ADR-0092; `NAVIGATOR_COMMAND_TARGETS` → ADR-0093;
  `REQUEST_REBUILD` and `REGISTRY_STACK`'s `ManuallyDrop` form → ADR-0094; `TIME_DILATION`
  → this ADR, as a property of each presentation's frame clock), or a permanent
  `grant = "ADR-0097"` with a `class` the gate checks:
  - `trampoline` — the one named exception above; it must be a `thread_local!` entry.
  - `counter` — an atomic integer advanced through a reference (a `try_update` helper behind
    `&NAME`, which keeps exhaustion an error); no direct `store`, `swap`, `load`, `fetch_sub`,
    bit-op, `fetch_min`, `compare_exchange*`, `get_mut`, `into_inner` or `as_ptr` on it.
  - `immutable` — its type names no `Cell`, `RefCell`, `UnsafeCell`, `SyncUnsafeCell`,
    `OnceCell`, `Mutex`, `RwLock`, `Condvar`, `Once`, `Barrier` or atomic; `OnceLock`,
    `LazyLock` and `LazyCell` only as the outermost type (a write-once canonical value).
  - `process` — mirrors a resource the process has once: an OS registration, the system
    clipboard, a GCD queue, tracing's global dispatcher. Nothing structural is checked; the
    reason names the resource.
  - `diagnostic` — a `Once` or `AtomicBool` flag, or an item whose cfg is false in every build
    without `debug_assertions` (so not `not(debug_assertions)`, nor
    `any(debug_assertions, …)`), that no behavior reads.

### 3. The allowlist lives in the manifests, is seeded by the scan, and only shrinks

- Entries go in each crate's `[package.metadata.flui]` as `globals`, following #1283's move of
  rules into the manifests; `globals` joins the keys `cargo xtask workspace` accepts
  (`FLUI_KEYS` in `tools/xtask/src/workspace.rs`). The schema is strict: exactly one of `exit`
  (an ADR with a file) and `grant`, a class only with a grant, a non-empty reason, no
  duplicate item, no other key.
- The PR that adds the gate seeds the allowlist from the scan's own output, in the same PR —
  not from this ADR's table, and not from regex counts.
- The gate fails on a finding with no entry (a new global) **and** on an entry with no finding
  or for an exempt one (a removed global whose entry was left behind), so the list can only
  shrink. A moved module changes the key; the report pairs the new global with the stale entry.
- `cargo xtask globals --seed` prints the skeleton of every unlisted global per crate; it
  writes nothing, and its empty reasons do not parse until filled in.

### 4. It reaches the merge path and can fail

`globals --self-test` runs the rules over in-memory crates (`tools/xtask/src/globals/fixture.rs`)
that plant an interior-mutable `static`, a `thread_local!` with no entry, an atomic that is
`store`d, a `pub` counter, a counter borrowed as `&NAME`, a static under
`cfg(any(test, feature = …))`, statics inside a `macro_rules!` body, an unknown macro and a
`quote!` body (`static #name`), a second host trampoline, trampolines outside the host and the
backends, a stale entry and an entry for an exempt counter, beside silent cases (a private
`fetch_add` counter, a `&str`, items and a whole module under `#[cfg(test)]`, a `'static`
lifetime in macro tokens, `(&'static $t:ty)` included), and fails unless
exactly the planted findings come back. Both `globals --self-test` and `globals` join the
in-process list of `cargo xtask checks`, and the test that pins that list
(`the_in_process_checks_include_the_link_check_under_strict` in `checks.rs`) names them.

### Why a syn scan and not a type or a lint

ADR-0078 moved rules into types and clippy and rejected regex scanners and dylint. There is no
type that makes an interior-mutable `static` unwritable, and stock clippy has no lint for one.
`clippy::disallowed_macros` can flag `thread_local!` but not `static`, and its per-site
`#[expect]` is local: a new exemption never passes through one reviewed list. A `syn` parse is
not a text match — a static behind a helper function or a type alias is still an item — and it
runs on the pinned stable toolchain, which dylint does not.

**Limits the gate does not claim to cover:**

- Environment-variable selection: `FLUI_HEADLESS` (`crates/flui-platform/src/lib.rs`),
  `FLUI_SELF_CLOSE_AFTER_MS` and `FLUI_SELF_CLOSE_ROUTE`
  (`crates/flui-platform/src/platforms/winit/platform.rs`), `FLUI_DX12_NO_DCOMP`
  (`crates/flui-engine/src/renderer.rs`) choose process-wide behavior without any static.
- Build-script output pulled in through `include!(concat!(env!("OUT_DIR"), …))`, as the six
  `crates/flui-engine/src/*/generated.rs` files do: the scan reads the source tree, not
  `OUT_DIR`. Any other `include!`, and a `mod x;` inside a fn, impl or block body, is an error
  rather than a file the scan never reads.
- Expansions of dependency macros and proc-macros. FLUI's own `macro_rules!` and `quote!`
  bodies are token-scanned; `once_cell` is refused in workspace manifests by `deny.toml`'s
  std-replacements check.
- State held in dependencies.
- `const` items, which are not places; clippy's `declare_interior_mutable_const` covers the
  interior-mutable ones.
- Determinism: an exempt counter is isolated from configuration, but an ID that reaches a
  snapshot is still process-ordered (`design/architecture.md`, "Process-global state").

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

- `cargo xtask globals --self-test` reports exactly the planted findings (§4);
  `cargo test -p xtask globals` pins the self-test both ways and the scan, cfg, counter and
  allowlist rules.
- `cargo xtask globals` is green on the seeded allowlist and is part of `cargo xtask checks`,
  pinned by the checks-list test. Adding `static X: std::sync::Mutex<u8> = …;` to any scanned
  file, or removing a seeded entry, makes it exit 1.
- Each removal lands with a test that fails with the global back: two realms with different time
  dilation animate at different rates; two realms register different fonts and each shapes only
  with its own; a navigation intent reaches the realm that owns the router.
