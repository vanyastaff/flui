# Extending FLUI

What adding each kind of thing takes. The rules behind the rows are in
[AGENTS.md](../AGENTS.md) ("What the compiler and gates enforce", "Definition of Done").

| Adding | What it takes |
|--------|---------------|
| **Render object** (`RenderBox`/`RenderSliver`) | Implement in `flui-objects` (protocol in `flui-rendering`) → register in `RENDER_OBJECT_TYPES` → `harness_*` tests in `render_object_harness` → record a non-obvious decision in the crate's `## Mapping decisions` |
| **Widget** | `View`/`ViewState` in `flui-widgets` or the facade, backed by a render object → `SemanticsConfiguration` for assistive tech → a test that fails without it |
| **Behavioural widget** (a gesture that does something: swipe, reorder, dismiss, zoom) | A `GestureAlternatives` constructor argument whose commands are `Intent`s (semantics action, keys, `VisibleControl`, `ContextMenu` or a `Companion` widget that already draws the controls, ADR-0216) → with `ContextMenu`, room for the control that keyboard or assistive-technology focus reveals at the row's trailing edge (`MenuCause::Control`, at least the minimum target size; the way to the menu where the platform has no menu key or the adapter does not route `ShowContextMenu`, ADR-0216 §2), which the widget does not clip or cover → labels and announcements through `WidgetsLocalizations` with parameters, mirrored in RTL → its row of the ownership table (`assert_ownership_table`) and, if it scrolls or is scrolled, of the nested-scroll channel (ADR-0208, ADR-0209) → its `assert_gesture_alternatives` matrix row → the `flui-material`/`flui-cupertino` style wrappers |
| **Text-editing widget** | Implement `flui_platform::TextStore` (embed a `LockArbiter`, `flui_sdk::interaction::text_input`'s, and pass it the `CommitGate` that `set_commit_gate` receives; the store keeps no transaction flag of its own), attach it through `TextInputHandle::attach` while focused, which installs the presentation's frame-transaction gate → run owner code through `OwnerCalls` → pass `flui_testing::text_store_kit::assert_conforms` (ADR-0090, ADR-0142) |
| **Platform capability** (a new handle) | The capability seam ([ADR-0152](adr/ADR-0152-capability-seam-revised.md)): a `Capability` marker and its handle in the crate that owns the handle (as `flui-interaction` owns `ClipboardCapability`), on `flui_platform::capability` alone → a `CapabilityProvider` registered per UI runtime: built in by `flui-app` over the owner platform, by a `Plugin`, or by `Application::capability` as an override, on every platform → acquired through `ctx.capability::<C>()` in a lifecycle hook, never in `build`; an answer that arrives later is a `Completion` settled at an owner turn → host code in `flui-native` with no platform types leaking out → a test through `HeadlessHost::with_capabilities` or a scripted provider under `testing` that fails without it; a service outside the core host's closed list is its own crate ([ADR-0154](adr/ADR-0154-capability-crates.md)) |
| **OS-specific window feature** (the escape hatch, [ADR-0254](adr/ADR-0254-window-contract.md) §8) | A method of that OS's window view in `flui-native` (`Win32Window`, `AppKitWindow`) that takes and returns FLUI types only and does what it says or returns a typed error saying why not (`Win32WindowError`, `MacOSBackdropError`; no success without the work, no logged failure) → it is reached through the OS's extension of `dyn HostWindow` (`win32()`, `appkit()`), so another host's window answers `None`, and an application reaches the window through `flui::native::with_window` → a new public name goes into `flui_native::os::<os>`, which the facade publishes as `flui::native::<os>` behind `native`, with a line in the facade's `tests/surface.rs` → an option the window is created with is a field of that OS's type in `flui_platform::os_options` (`#[non_exhaustive]`), read by that OS's host alone → a case in `flui-native`'s `tests/os_extensions.rs` that the OS's host does it and a headless window has no view; an API newer than the OS floor is looked up at run time (a class by name, a function by symbol) or gated in `version/`, and answers a typed error where it is absent |
| **Platform** (a new operating environment for the core host) | std's word for it (`tvos`, `ohos`, `fuchsia`, a session under `linux/`) → a file of that name in each feature of `flui-native` it fills, its pure part a module under `cfg(any(<os>, test))` with `forbid(unsafe_code)` and its FFI under `cfg(<os>)` → its table `hosts/<os>.rs`: `impl Host` and a constructor, starting with `Absent<F>` in every position it does not fill (its rows of `docs/platform-parity.md` are read off the table; it is a ready platform when no `Absent` is left where parity marks the position applicable and the conformance rows ran there) → its floor in `platform-support.toml` and its version type in `flui-native`'s `version/gate.rs` (ADR-0244), its `docs/gaps.md` entries, a CI target and its `globals` entries. A host outside FLUI waits for an embedder seam, not for a crate split ([ADR-0151](adr/ADR-0151-platform-layer-boundary-and-names.md) §10) |
| **Crate** | A workspace `members` entry and `[package.metadata.flui]` `tier`, `tier-kind` and `order` (the tier names live in root `[workspace.metadata.flui] tiers`); `cargo xtask workspace` checks the rest. Why a crate is a boundary: `docs/crates.md` "Adding a New Crate", [ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md) |
| **Official package** | Under `packages/<name>/`, `tier = "pkg"`, `tier-kind = "official"`, no `edge-exceptions`; its only FLUI normal dependency is `flui-sdk` (plus the contract crates), and its code names the framework as `flui_sdk::…` (the view, inherited, animation and `Inspect` derives resolve through it). An item the SDK lacks is added to `flui-sdk` by ADR-0088 §4, with a line in its `tests/surface.rs` pinned list |
| **Example using `material`/`cupertino`** | `[[example]] required-features = [...]` (`cargo xtask facade-combos` relies on it) |

## A widget's constructor and names

Every widget in `flui-widgets`, `flui-material` and `flui-cupertino` has the shape its Flutter
counterpart has, adapted to Rust:

- **Child.** A child that Flutter's constructor marks `required` is an argument of `new`; a child
  that Flutter leaves nullable is set with `.child(…)`, and the widget lays out, paints and
  hit-tests without one. A forgotten required child is then a compile error, and an optional one
  is never asked for. A multi-child widget takes its children in `new`. The child or children
  come last among the arguments of `new`, after the other required values (Flutter's
  `sort_child_properties_last`). A widget with no Flutter counterpart asks the same question:
  does it mean anything without its child?
- **Callbacks.** A callback field and its builder method carry Flutter's name in snake case:
  `on_pressed`, `on_long_press`, `on_changed`, `on_submitted`, `on_tap`. A callback Flutter
  declares `required` but nullable (`VoidCallback? onPressed`) is a builder method; leaving it
  out is Flutter's `null`.
- **Contexts.** A context is bound as `ctx`, whatever its kind: `BuildContext`,
  `LifecycleContext`, `RenderObjectContext`, the layout, paint and hit-test contexts, a
  capability's `ProviderContext`, an event callback's context and a `poll`'s
  `std::task::Context`. A context type is named `…Context`. `cargo xtask context-names` holds the
  binding ([ADR-0325](adr/ADR-0325-a-context-is-bound-as-ctx-and-a-description-builder-has-no-with.md)).
- **Builder methods.** An optional parameter of the Flutter constructor is a builder method with
  the parameter's bare name: `.alignment(…)`, never `.with_alignment(…)`. The same holds on every
  description the author assembles and hands over (`Application`, `AppConfig`, a
  `…Options/Settings/Request/Builder/Params/Spec`). A parameter a host reads back is a `pub` field
  of a `#[non_exhaustive]` type, never a `get_x`. `with_x` stays where Flutter has `withX` or a
  setter: a finished value (`Color::with_alpha`), a render object's chaining pair of `set_x`.
  `cargo xtask builder-names` holds the rule ([ADR-0325](adr/ADR-0325-a-context-is-bound-as-ctx-and-a-description-builder-has-no-with.md)).
- **Units.** A type generic over `Unit` defaults to logical pixels, `T: Unit = f64`, as `Size`,
  `Rect` and `Offset` do, so a signature writes `BoxDecoration`, not `BoxDecoration<f64>`.

`flui-widgets`' `child_form` table test holds a row for each widget whose form was ever in
doubt; a widget that changes form changes its row.
