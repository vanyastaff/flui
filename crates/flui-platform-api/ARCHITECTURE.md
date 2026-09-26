# flui-platform-api Architecture

The contract half of the platform layer ([ADR-0082](../../docs/adr/ADR-0082-platform-api-contract-crate.md)).
`flui-platform` holds the backends and re-exports every item here at its old
path.

## Invariants

- **No backend types.** No OS (`windows`, `objc2-*`, `android-activity`,
  `ndk`, `web-sys`), winit, AccessKit or tokio type appears in a signature,
  and none of those crates is a dependency. `flui-platform`'s
  `allowed-dependents` and the `cargo tree` probe in ADR-0082 §3 are what
  keep that true for the crates above.
- **No `unsafe`.** `#![forbid(unsafe_code)]`: FFI belongs to the backends.
- **Flat root.** Every public item is re-exported at the crate root; the
  modules are private except `data_transfer`, whose many vocabulary types keep
  their module path (`flui_platform::data_transfer` re-exports it whole).
- **`ui-events` re-exports are debt.** `PlatformInput` wraps the `ui-events`
  pointer and keyboard types and re-exports them (with `keyboard-types`' `Key`
  and `Modifiers` through `ui-events`). ADR-0089 keeps upstream types out of
  stable signatures; this crate's own input types replace them before its
  first release.
- **`PlatformWindow` is not here yet.** It still returns
  `PlatformAccessibility`, whose signatures are AccessKit's, and carries the
  winit-only `as_winit`. It moves once `accessibility()` goes to a host-side
  subtrait (ADR-0082 §3, second change).

## Mapping decisions

### Flutter's `services` layer becomes capability traits in a contract crate

Flutter has no equivalent crate. Its `services` library carries text input,
haptics, clipboard and system chrome as method-channel messages to one
embedder. FLUI dissolves that layer into typed capability traits
(ADR-0030, ADR-0031, ADR-0038) and keeps the traits apart from their
implementations, so a crate that names a capability links no OS code and a
plugin can implement one without the backends. The observable contracts of
each capability are those of its own ADR; only where the trait is defined
changed.
