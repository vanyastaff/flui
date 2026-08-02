# Workspace Boundary and Logging Review

> Pre-Runtime.1 review of workspace members, crate responsibilities, facade
> composition, localization direction, and logging ownership.

**Date:** 2026-08-01
**Status:** accepted pre-sprint direction; implementation remains tracked work.
**Scope:** crate boundaries and composition only. This does not redesign the
Flutter-loyal View, Element, or Render behavior.

Related documents:

- [Runtime Architecture Execution Plan](2026-08-01-runtime-architecture-execution-plan.md)
- [Runtime Dependency Adoption Guide](2026-08-01-runtime-dependency-adoption-guide.md)
- [Crate Decomposition Redesign](2026-05-22-crate-decomposition-redesign.md)
- [Foundations](../FOUNDATIONS.md)

## Executive decision

The workspace is not generally over-split. Cargo resolves an acyclic graph,
the inventory gate covers all active library crates, and most members hide a
substantial responsibility. Runtime.1 should not begin with a broad crate
rewrite.

Five boundaries do need correction before or at the start of the milestone:

1. logging emission, cross-platform backend assembly, and global subscriber
   installation are separate concerns;
2. runtime execution ownership must leave `flui-platform`;
3. `flui-app` must stop making development capabilities unconditional;
4. localization and facade dependency directions need an explicit contract;
5. the headless test driver needs a name that cannot be confused with the
   transitional Flutter-style binding graph.

The scheduler and runtime composition will then be narrowed by their existing
Runtime.1 work rather than by speculative crate creation.

## What the workspace audit checked

The review used Cargo metadata to separate normal and development edges,
inspected each active member's manifest and reverse consumers, compared the
actual graph with `docs/FOUNDATIONS.md` and `docs/crates.md`, and examined the
production sites that cross the disputed boundaries.

Important observations:

- The root facade always compiles both `flui-material` and `flui-cupertino`.
- `flui-app` has normal edges to nearly every runtime layer and to
  `flui-hot-reload`.
- Every desktop platform constructs its own `BackgroundExecutor`.
- `flui-localizations` currently implements only the widget-localization
  contract and has no in-workspace production consumer.
- `flui-binding` is a deterministic headless test driver; catalog crates use it
  only as a development dependency.
- `flui-foundation` emits tracing events but also installs the process-global
  subscriber and owns platform logging backends.
- `flui-view -> flui-objects` is a real production edge for special paired
  element/render implementations such as layout builders and lazy slivers. It
  is not a cycle; the documented layer placement is stale.

`cargo metadata` and the inventory script prove graph consistency. They do not
prove that a responsibility is in the right crate, so the corrections below
remain necessary despite a green inventory gate.

## `flui-log` history

The standalone `flui-log` crate provided a `Logger` builder over
`tracing-subscriber`, optional `tracing-forest` formatting, and native sinks for
Android logcat, Apple unified logging, and the browser console. Its public API
was almost a direct restatement of subscriber configuration.

The May 2026 crate-decomposition research classified it as shallow: the crate
added a manifest and widely repeated dependency edge while hiding little. Its
one substantial asset was the platform sink implementation, which did not by
itself justify a universally depended-on crate.

Commit `e3a3c4ff4cdeb40f388c13b5dfd593aab5226574` merged the implementation into
`flui_foundation::log` and removed the member. The migration was deliberately
mechanical:

- copy the logger and platform layer into foundation;
- rewrite consumers from `flui_log` to `flui_foundation::log`;
- remove the standalone member after it had no production consumers.

That decision correctly removed a shallow, universally depended-on crate. It
did not invalidate the cross-platform backend itself, and it did not reconsider
the internal responsibility of every moved item. As a result, foundation now
owns both sides of a boundary that a hostable runtime must keep distinct.

The historical implementation also carried deliberate future-facing behavior:

- automatic desktop, Android, iOS, and web backend selection;
- application identity for logcat tags and Apple subsystem names;
- module filters and environment overrides;
- optional hierarchical span output;
- browser console and performance-timeline integration;
- one place to extend platform privacy, buffering, and export policy.

These capabilities are retained. The correction changes their ownership, not
their product direction.

## Logging has two different responsibilities

### Library-side instrumentation

Framework libraries emit structured events and spans. This is dependency-safe:

```rust,ignore
tracing::info!(presentation = %id, generation, "presentation committed");
```

Libraries must not decide where those events go. They may depend on `tracing`,
define stable field names, redact sensitive values, and expose diagnostic data
types. This responsibility is appropriate for foundation and the owning
subsystems.

### Process-side collection

Installing a subscriber mutates process-global state and selects output,
filtering, formatting, native sinks, trace export, and privacy policy. It is an
application or host decision, not a foundation primitive.

Today `Logger::init` performs that mutation from `flui-foundation`, while both
`run_app` and `run_direct` invoke it automatically and panic if another
subscriber is already installed. This is incompatible with embedding FLUI in a
game engine, editor, test process, or service that already owns observability.

## Target diagnostics ownership

Restore `flui-log` only with a stricter composition-only responsibility. It is
not a universal facade for framework libraries and must not become a dependency
of View, rendering, widgets, engine, or other event producers. It is the
cross-platform default backend selected by managed entry points.

This makes the crate deeper than its former shape: it hides the platform matrix,
subscriber composition, filter policy, application identity, native privacy
rules, and future sink/export integration behind a small setup API. Libraries
continue to use `tracing` directly, so removing or replacing the default backend
does not change instrumentation call sites.

Apply the following ownership model:

| Concern | Owner |
|---|---|
| Event/span emission and shared diagnostic value types | the subsystem plus `flui-foundation` primitives |
| Cross-platform default backend and native sink construction | restored, composition-only `flui-log` |
| Managed `run_app` default subscriber policy | `flui-app` composition root calling `flui-log` |
| Embedded runtime subscriber policy | the host; FLUI inherits it and never replaces it |
| CLI subscriber policy | `flui-cli`, reusing `flui-log` backend construction |
| Timeline, inspector, trace export, and profiler consumers | `flui-devtools` adapters over `tracing` |
| User-visible production fallback | presentation error policy, not logging |
| Crash persistence/upload | a future explicit application service, not the log API |

The managed entry point should be convenient without taking hostile ownership:

- `Auto`: preserve an existing subscriber; otherwise install FLUI's platform
  default;
- `Inherit`: never install a subscriber, even when none exists;
- explicit application configuration: install the requested layers and return
  a typed error if that is impossible.

The embedded entry point defaults to `Inherit`. It must never silently replace
or globally narrow a host's subscriber. Exact public names remain subject to
the Runtime.1 API gate; the behavior above is the contract.

Foundation keeps `tracing` emission but should lose normal dependencies on
`tracing-subscriber`, `tracing-forest`, `android_log-sys`, `tracing-oslog`, and
`tracing-wasm` once backend construction has moved to `flui-log`. Tests may use
subscriber utilities as development dependencies where they capture events.

The restored crate must preserve or improve the historical platform behavior:

- desktop `fmt` and optional hierarchical output;
- Android logcat tags and structured fields;
- Apple unified logging with deliberate subsystem, category, and privacy rules;
- browser console and performance-timeline spans;
- `RUST_LOG`/module filtering without a hard-coded ceiling that suppresses
  requested trace events;
- non-panicking detection of an existing subscriber;
- correlation fields for runtime, realm, presentation, frame, and worker IDs;
- future dynamic filter reload and additional sinks without changing framework
  crates.

Crash capture, remote upload, and the user-visible error surface remain separate
services. `flui-log` may emit or export diagnostics for them, but must not own
their lifecycle or product policy.

## Workspace member decisions

### Keep without structural change

The following boundaries are justified and should not be churned during the
runtime sprint:

- `flui-geometry` separate from the broader value catalog in `flui-types`;
- `flui-tree` as common tree protocol and arity vocabulary;
- `flui-painting`, `flui-layer`, and `flui-semantics` as distinct retained
  pipeline products;
- `flui-rendering` separate from the concrete `flui-objects` catalog;
- `flui-view` separate from the author-facing `flui-widgets` catalog;
- `flui-engine` separate from OS-facing `flui-platform`;
- `flui-animation`, `flui-assets`, `flui-macros`, `flui-build`, `flui-cli`, and
  `flui-devtools` as independently substantial responsibilities;
- Material and Cupertino as sibling terminal design-system crates;
- target validation examples as workspace members but not default members;
- Android packages outside the workspace while they require an external NDK
  bootstrap.

Do not create a standalone physics, services, or signals crate. `flui-log` is
the deliberate exception: it is restored as a composition-only cross-platform
backend, not as a dependency used by every library to reach logging macros.

### Correct the documented `flui-objects` placement

The actual production graph is:

```text
flui-rendering <- flui-objects <- flui-view <- flui-widgets
```

`flui-view` legitimately names concrete render types for framework machinery
whose element and render halves cooperate. Therefore `flui-objects` is the
render catalog between the render machine and View, not a peer placed above
View beside the widget catalog. Correct the diagrams and layer checks; do not
invert the code merely to match a stale diagram.

### Keep `flui-localizations`, correct its future direction

The crate is shallow today because it implements only
`GlobalWidgetsLocalizations`, but generated and handwritten translations will
make it large. Its separation is justified.

Contracts and default English implementations belong to their defining
catalogs. Global translations implement those contracts:

```text
flui-localizations -> flui-widgets
flui-localizations -> flui-material      (when GlobalMaterialLocalizations lands)
flui-localizations -> flui-cupertino     (when GlobalCupertinoLocalizations lands)
```

Material and Cupertino must not depend back on `flui-localizations`, or the
implementation package and its interface owners form a cycle. The facade can
offer an optional `localizations` feature and re-export the package without
changing these internal directions.

### Make the facade feature-selective

The facade currently compiles both design systems unconditionally. Target
shape:

```toml
[features]
default = ["material"]
material = ["dep:flui-material"]
cupertino = ["dep:flui-cupertino"]
localizations = ["dep:flui-localizations"]
hot-reload = ["flui-app/hot-reload"]
```

The exact default is an author-experience choice, but unused Cupertino,
localization data, and hot-reload machinery must be removable. Feature-matrix
CI must cover each supported combination and reject feature wiring that only
works through workspace unification.

### Rename `flui-binding` to `flui-testing`

The crate is a headless deterministic frame driver used by tests. The name
`binding` collides conceptually with the transitional `AppBinding` graph that
Runtime.1 removes. `flui-testing` gives a stable home for `WidgetTester`, the
virtual clock, fake platform capabilities, deterministic replay, golden-image
support, and test-only protocol drivers.

The rename should happen before Runtime.1 adds more conformance harness APIs,
otherwise the old package name becomes a public testing contract.

### Narrow `flui-app`, do not extract `flui-runtime` prematurely

Before the milestone, make hot reload optional and remove parked theme or
direct-render surfaces that have no supported runtime path. During the
milestone, `flui-app` is the private composition root for runtime ownership.

Extract `flui-runtime` only after both managed and host-driven entry points use
the same proven core and the extraction removes a real dependency problem. A
crate created before those two consumers exist would merely freeze a guessed
boundary.

### Narrow scheduler and platform responsibilities in existing work

Keep `flui-scheduler`, but reduce it to logical update phases, tickers, callback
ordering, and owner-local post-frame behavior. Presentation clocks and raster
backpressure belong to presentation/runtime ownership; workers and durable
services belong to runtime execution.

Remove `BackgroundExecutor` and `PlatformExecutor` from `flui-platform` as the
host-injected execution task lands. Platform retains the event loop, native
windows, input, display timing, and OS capabilities. Web-specific task spawning
is an implementation of the runtime's default executor, not a reason for every
Platform object to own an executor.

## Pre-sprint gate

The following preparation precedes the existing Runtime.1 conformance task:

1. **Workspace topology contract ([#567](https://github.com/vanyastaff/flui/issues/567)).**
   Correct diagrams and mechanically validate
   critical forbidden dependency directions. Record the extraction gate for a
   possible future `flui-runtime` crate.
2. **Diagnostics composition boundary ([#568](https://github.com/vanyastaff/flui/issues/568)).**
   Restore the cross-platform logging backend outside foundation, preserve its
   platform roadmap, and prove managed auto-setup plus embedded host
   inheritance.
3. **Public package surface cleanup ([#569](https://github.com/vanyastaff/flui/issues/569)).**
   Feature-gate design systems,
   localizations, and hot reload; rename the headless driver to `flui-testing`;
   remove or explicitly classify parked application APIs.

These tasks are structural preparation, not permission to alter View, Element,
Render, reconciliation, layout, paint, or lifecycle behavior.

## Acceptance evidence

- Foundation can be used by an embedded host without installing or linking a
  subscriber backend.
- The restored `flui-log` is depended on only by composition roots and retains
  desktop, Android, Apple, and web backend behavior.
- Managed startup preserves an existing subscriber and installs a default only
  when policy allows it.
- Embedded startup never changes global subscriber ownership.
- Native log sinks have target-specific compile checks and at least one real-OS
  smoke path where CI or external hardware permits it.
- The facade builds with Material only, Cupertino only, both, and neither where
  the documented API permits those combinations.
- Localization dependencies remain acyclic when Material and Cupertino global
  translations are added.
- Runtime conformance tests import `flui-testing`, not the transitional binding
  package name.
- Inventory checks compare the declared layer policy with normal Cargo edges,
  not only member names and metadata inheritance.

## Historical sources

- Git commit `e3a3c4ff4cdeb40f388c13b5dfd593aab5226574`, which merged and removed
  `flui-log`.
- [Crate Decomposition Redesign](2026-05-22-crate-decomposition-redesign.md),
  which correctly identified the old crate as shallow.
- [D-block Implementation Plan](../plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md),
  which intentionally performed a mechanical move and therefore did not
  re-evaluate composition-root ownership.
- Flutter's localization package structure and delegate composition:
  <https://api.flutter.dev/flutter/flutter_localizations/>.
