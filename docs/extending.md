# Extending FLUI

What adding each kind of thing takes. The rules behind the rows are in
[AGENTS.md](../AGENTS.md) ("What the compiler and gates enforce", "Definition of Done").

| Adding | What it takes |
|--------|---------------|
| **Render object** (`RenderBox`/`RenderSliver`) | Implement in `flui-objects` (protocol in `flui-rendering`) → register in `RENDER_OBJECT_TYPES` → `harness_*` tests in `render_object_harness` → record a non-obvious decision in the crate's `## Mapping decisions` |
| **Widget** | `View`/`ViewState` in `flui-widgets` or the facade, backed by a render object → `SemanticsConfiguration` for assistive tech → a test that fails without it |
| **Behavioural widget** (a gesture that does something: swipe, reorder, dismiss, zoom) | A `GestureAlternatives` constructor argument whose commands are `Intent`s (semantics action, keys, `VisibleControl` or `ContextMenu`, ADR-0216) → labels and announcements through `WidgetsLocalizations` with parameters, mirrored in RTL → its row of the ownership table (`assert_ownership_table`) and, if it scrolls or is scrolled, of the nested-scroll channel (ADR-0208, ADR-0209) → its `assert_gesture_alternatives` matrix row → the `flui-material`/`flui-cupertino` style wrappers |
| **Text-editing widget** | Implement `flui_platform_api::TextStore` (embed a `LockArbiter` and pass it the `CommitGate` that `set_commit_gate` receives; the store keeps no transaction flag of its own), attach it through `TextInputHandle::attach` while focused, which installs the presentation's frame-transaction gate → run owner code through `OwnerCalls` → pass `flui_testing::text_store_kit::assert_conforms` (ADR-0090, ADR-0142) |
| **Platform capability** (a new handle) | Trait in `flui-platform-api`, backend in `flui-platform` with no platform types leaking out → a method on `LifecycleContext`, not `BuildContext`, so `build` cannot reach it → a test that fails without it → ADR if it changes a cross-crate contract |
| **Crate** | A workspace `members` entry and `[package.metadata.flui]` `tier`, `tier-kind`, `order` and `layer = N` (names: root `[workspace.metadata.flui] tiers` and `layers`); `cargo xtask workspace` checks the rest. Why a crate must be a layer: `docs/crates.md` "Adding a New Crate", [ADR-0041](adr/ADR-0041-workspace-topology-contract.md) |
| **Official package** | Under `packages/<name>/`, `tier = "pkg"`, `tier-kind = "official"`, no `edge-exceptions`; its only FLUI normal dependency is `flui-sdk` (plus the contract crates), and its code names the framework as `flui_sdk::…` (the view, inherited and animation derives resolve through it; `Diagnosticable`'s has no SDK path yet). An item the SDK lacks is added to `flui-sdk` by ADR-0088 §4, with a line in its `tests/surface.rs` pinned list |
| **Example using `material`/`cupertino`** | `[[example]] required-features = [...]` (`cargo xtask facade-combos` relies on it) |
