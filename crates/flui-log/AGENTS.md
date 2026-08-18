# AGENTS.md — flui-log

Cross-platform logging **backend**. It assembles a `tracing` subscriber; it is
not how framework code reaches the logging macros.

## The one rule that matters

**Only `flui-app`, `flui-cli`, and the `flui` facade may depend on this crate.**

Every other crate — View, rendering, objects, widgets, engine, painting,
platform, everything — depends on `tracing` and nothing else. They emit
structured events and hold no opinion about where those events go, so removing
or replacing the default backend must never touch an instrumentation call site.

This is enforced mechanically, not by convention: `docs/workspace-layers.toml`
gives this member an `allowed_dependents` list, and `just inventory-check` (part
of `just ci` and the CI `checks` job) fails on any other normal edge into it.
The crate's low layer number reflects that it has *no* in-workspace
dependencies; it is not permission for the workspace above it to depend on it.

A predecessor crate was deleted in `e3a3c4ff` for becoming exactly that: a
shallow, universally depended-on wrapper. It came back with a narrower
responsibility, not a wider one.

## What lives here

| Module | Owns |
|---|---|
| `identity` | `AppIdentity`, `AppleBundleId` — the names the native sinks index by |
| `filter` | `FilterConfig` → one `EnvFilter`, and nothing else |
| `ownership` | setup/install policies, installation outcome, and the `log` compatibility bridge |
| `config` | `LogConfig`, `DesktopFormat`, and the default subscriber stack |
| `backend` | `PlatformLayer` and the per-target sinks |

## Invariants you can break by accident

- **No second level ceiling.** There is deliberately no `Level` knob anywhere in
  the API, and every native backend is constructed wide open (`WASMLayer`'s
  `set_max_level(TRACE)`, `PlatformLayer::max_level_hint` forwarded verbatim).
  The historical logger stacked a `LevelFilter` seeded from a field defaulting
  to `INFO` beside the `EnvFilter`, so `RUST_LOG=flui_view=trace` selected
  events that were then discarded. Adding any level parameter reintroduces it.
- **Filters belong to sinks.** Attach the native `EnvFilter` with
  `PlatformLayer::with_filter`; do not place it below the entire registry. A
  global filter would silently clip independently configured timeline,
  metrics, or capture layers.
- **A display name is not a bundle identifier.** `AppIdentity` keeps them
  separate on purpose. The historical code synthesised `com.{display_name}.app`,
  which produced illegal identifiers and could file a FLUI app's logs under a
  reverse-DNS name somebody else owns. An application without a declared bundle
  identifier gets the fixed `UNIDENTIFIED_APPLE_SUBSYSTEM`, never a guess.
- **Nothing here panics on a taken subscriber slot.** `Install` returns
  `SetupError::SubscriberAlreadyInstalled`; `Auto` returns
  `SubscriberOwnership::Unchanged`. An embedded host owns its own observability.
- **An existing `log` logger is host-owned.** Install `LogTracer` only after
  FLUI wins the tracing subscriber slot. If another logger already exists,
  preserve it and report `LogBridgeStatus::ExistingLoggerPreserved`.
- **The two global slots have separate permission.** A host may let FLUI install
  tracing while reserving the `log` facade for later. Preserve
  `LogBridgePolicy::Inherit`; it must not inspect or claim the logger slot.
- **`Inherit` must not even read the global slot.** `setup` short-circuits
  before building a subscriber. If that changes, `tests/inherit_never_touches_the_global_slot.rs`
  is what catches it.
- **Do not reach for `tracing::dispatcher::has_been_set`.** It is `#[doc(hidden)]`
  upstream *and* it is raised by a thread-local `with_default`, so a single
  scoped subscriber anywhere in the process would convince `Auto` that the global
  slot was taken forever. `set_global_default`'s own `Result` is the exact
  signal, with no window between the question and the answer.
- **Device sinks are private by default — do not construct one bare.** The
  logcat and Apple sinks only exist wrapped in `backend::redact::RedactLayer`,
  which replaces every dynamic value (string, `Debug`/`Display` rendering,
  error) with `<private>` unless the field name ends in `.public`, and every
  scalar whose name ends in `.private`. A native `tracing` message publishes —
  so user content stays in fields, never interpolated into the sentence — but a
  message bridged from the `log` facade redacts: it is a third party's fully
  interpolated string, and no marker can vouch for it. The
  classification (`backend::privacy`) is Apple's own `%{public}`/`%{private}`
  default ported to both platforms; it and the redaction stage are compiled and
  unit-tested on every target. A new device sink joins the contract by being
  constructed wrapped — adding one bare reopens the leak #571/#572 closed.
  Desktop and web-console sinks are developer-facing and deliberately publish
  verbatim.
- **macOS is a desktop.** It keeps the `fmt` backend; unified logging there
  would make `cargo run` print nothing. `os_log` on macOS is opt-in through the
  `apple-unified-logging` feature.

## Testing

The process-global subscriber slot can be written once per process, so
`tests/` holds **one scenario per binary** and no binary has more than one test
that writes the slot. That holds under `cargo test` (process per file) as well
as `cargo nextest` (process per test), and no test depends on execution order.
Do not merge two slot-writing scenarios into one file to save a binary.

Backend logic that would otherwise only compile on a device — the logcat level
mapping, the tag fallback chain, the NUL handling, the field rendering — is
compiled on **every** target and unit-tested on the CI host, with only the FFI
call itself behind `cfg(target_os = "android")`. The historical Android tests
lived inside the `cfg` island, never compiled anywhere, and were asserting
behaviour the code did not have.

## Out of scope

Crash persistence, remote upload, and the user-visible error surface are
separate services. This crate may feed them; it must not own their lifecycle or
product policy.
