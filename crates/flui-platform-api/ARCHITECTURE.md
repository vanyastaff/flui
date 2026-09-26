# flui-platform-api Architecture

The contract half of the platform layer ([ADR-0082](../../docs/adr/ADR-0082-platform-api-contract-crate.md)).
`flui-platform` holds the backends and re-exports every item here at its old
path.

## Invariants

- **No backend types.** No OS (`windows`, `objc2-*`, `android-activity`,
  `ndk`, `web-sys`), winit, AccessKit or tokio type appears in a signature,
  and none of those crates is a dependency. `cargo xtask reach` holds it: tier
  C forbids the OS crates and winit, and this crate's own `reach-forbid` adds
  `accesskit` and `tokio`. `flui-platform`'s `allowed-dependents` keeps the
  crates above from reaching the backends through a side door.
- **No `unsafe`.** `#![forbid(unsafe_code)]`: FFI belongs to the backends.
- **Flat root.** Every public item is re-exported at the crate root; the
  modules are private except `data_transfer`, whose many vocabulary types keep
  their module path (`flui_platform::data_transfer` re-exports it whole).
- **`ui-events` re-exports are debt.** `PlatformInput` wraps the `ui-events`
  pointer and keyboard types and re-exports them (with `keyboard-types`' `Key`
  and `Modifiers` through `ui-events`). ADR-0089 keeps upstream types out of
  stable signatures; this crate's own input types replace them before its
  first release.
- **`PlatformWindow` has no `accessibility()` and no `as_winit`.** The
  accessibility bridge speaks AccessKit, so `flui-platform`'s
  `HostWindow: PlatformWindow` carries it host-side: `open_window` returns an
  `Arc<dyn HostWindow>`, and the runner reads the bridge once before handing
  the realm an `Arc<dyn PlatformWindow>`. A `compile_fail` doctest on the
  trait, paired with a twin that compiles, pins that the method is gone.
- **The raw-handle impls live with the trait.** `HasWindowHandle` and
  `HasDisplayHandle` for `dyn PlatformWindow` are here because the orphan rule
  puts them next to the trait; `flui-platform` repeats them for
  `dyn HostWindow`, so an `open_window` result is a renderer target before its
  upcast. `raw-window-handle` appears only through those traits and their
  `WindowHandle`/`DisplayHandle`/`HandleError` (ADR-0089).

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
