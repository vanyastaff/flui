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
  modules are private except two whose many vocabulary types keep their module
  path: `data_transfer` (`flui_platform::data_transfer` re-exports it whole)
  and `text_store`, whose four traits (`TextStore`, `TextStoreRead`,
  `TextStoreEdit`, `TextStoreObserver`) are also at the root.
- **The text store is owner-thread and UTF-16.** `TextStore` is shared as
  `Rc<dyn TextStore>`; `LockArbiter` and `InMemoryTextStore` are not `Send`
  (a `static_assertions` test pins it). Every offset on the surface is a
  `Utf16Offset`, and the conversion to a field's own representation is
  `text_store::utf16`, nowhere else.
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

### A TSF-shaped pull store replaces Flutter's `TextInputClient` push

Flutter's text input is a push protocol: the engine sends whole
`TextEditingValue`s to a `TextInputClient` over a method channel, and the
client pushes its value back. `text_store` inverts it (ADR-0090): the field is
a `TextStore` the platform locks, reads and edits, in UTF-16 offsets, the
shape of TSF's `ITextStoreACP` that AppKit's `NSTextInputClient` and Android's
`InputConnection` also map onto. A push source (winit's `ImeEvent`) goes
through `project_ime_event`, so a field has one editing path. `LockArbiter`
holds the lock rules once for every implementation, including the frame
transaction: it reads the `CommitGate` the store's owner installs through
`TextStore::set_commit_gate`, so no store keeps a transaction flag of its own
to forget. `flui_testing::text_store_kit` checks a store against these rules.
**Tests:** the `text_store` module's unit tests, and `flui-testing`'s
`in_memory_store_conforms_to_kit_v1`.
