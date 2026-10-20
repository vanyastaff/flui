# flui-sdk Architecture

The package-author surface of
[ADR-0088](../../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) §4: one crate that an
official or third-party package depends on instead of the internal crates, so the core's crate
topology is not part of any package manifest.

## Invariants

- **Host-free.** The normal dependency closure names none of `flui-app`, `flui-engine`, `wgpu`
  or the platform backends: tier K's forbid set in the root `[workspace.metadata.flui.reach]`,
  checked by `cargo xtask reach` over this crate's default and all-features builds.
- **Same items, not copies.** Every path is a `pub use` of the item itself: a whole crate
  (`pub use flui_widgets as widgets`) or one item (`pub use flui_painting::Canvas`). No wrapper,
  newtype or re-declared trait, so `flui_sdk::m::T` and `flui::m::T` are one type wherever the
  facade has that path. `tests/surface.rs` (`the_re_exports_are_the_facades_types`) fails to
  build if a curated item or a whole-module representative stops being the facade's type.
- **Facade paths.** A module that the facade also has sits at the facade's path and holds a
  subset of it; an item the facade does not expose goes into an Evolving module, never into a
  facade-named one.
- **The composition-root seam is private.** The SDK's `view` module shadows
  `flui_view::__runtime`. `trybuild_ui::ui_tests` in
  `tests/compiler_guards.rs` checks the exact E0603 diagnostic when a package
  imports that module, and a passing fixture names the actual runtime trait
  through `flui-view` together with the public SDK view trait. An unrelated
  unresolved import cannot satisfy that diagnostic snapshot.
- **The list is pinned.** `tests/surface.rs` (`the_public_surface_is_the_measured_list`) compares
  the `pub use`/`pub mod` lines of `src/lib.rs` with a pinned list, so adding an item is a
  visible decision, and its `measured` module names every measured item through its SDK path,
  so removing one fails to build.
- **Evolving surface under the ceiling.** `pipeline` and `hooks` are Evolving. The four
  `pipeline` items count toward the ceiling; `hooks` holds one (`FrameSnapshot`) and, like every
  development hook, sits outside it. ADR-0088 §4 revisits the crate when its Evolving surface
  beyond the hooks passes about thirty items.
- **Own version.** `version = "0.N"`, not the workspace's: `cargo xtask workspace` refuses an
  evolving crate that inherits the version or leaves major 0.
- **One train per graph.** The guard is not here: `flui-foundation` declares
  `links = "flui_train"`, because the facade does not depend on this crate and a guard here
  could not separate an application on one train from a package on another. `cargo xtask reach`
  states that `flui-foundation` is in this crate's build and in the facade's.
- **The facade's test edge only.** `flui` is a path-only dev-dependency for the identity test;
  `cargo package` drops it, and nothing in `src/` names the facade.

## The measured surface

`painting::SemanticsRegion` and `painting::SemanticsRegionKey` name the values
returned by `widgets::CustomPainter::semantics_regions` (ADR-0223). A package
can implement that trait using only the SDK, with configuration from the
whole `widgets` surface. The measured list and facade identity test pin both
values.

`animation::AnimationBehavior` is used by essential timer and loading-indicator
controllers. `animation::MotionPolicy` is read by Cupertino route composition
through `widgets::MediaQuery::motion_of` (ADR-0195). These consumer imports are
pinned in `tests/surface.rs`.

`painting::TextSizing` is the existing Painting policy carried by
`widgets::MediaQueryData::text_sizing`. Package authors can choose fixed or
linear subtree sizing through the SDK; native capture construction stays with
the host. Numeric growth profiles and authored sizing intent are exposed through
the whole `foundation` re-export. The surface and identity tests pin the policy
gateway alongside `TextStyle`.

`platform::InvalidLocale` names the error returned by the validated `Locale`
constructors and parser (ADR-0173). Package authors can handle malformed authored
language tags without depending on an internal crate. The surface and identity
tests pin this export alongside the locale value.

`platform::TargetPlatform` keys Material's `PageTransitionsTheme` and its
`ThemeData::platform` (ADR-0221 D3). The page-transition hook it feeds,
`widgets::PageTransitionBuilder`, `widgets::PageTransitions` and the shared
`widgets::ParallaxPageTransition`, arrives through the whole `widgets`
re-export; the surface test pins all four and the identity test the platform value.

`interaction::FocusSubscription` exposes the existing owner-affine RAII token
for Material TextField's effective-node observer. The package acquires it with
`Rc<FocusNode>::subscribe`, publishes replacement ownership before retiring the
previous token, and withdraws by Drop. It does not regain the private listener
ID mutation methods or depend directly on flui-interaction. The surface list
and measured import in `tests/surface.rs` pin this gateway (ADR-0088 §4;
ADR-0165).

The items are what `flui-material` and `flui-cupertino` import from the internal crates outside
their tests, measured at `431c8757c` (2026-09-26):

1. every `flui_*::` path under `crates/flui-{material,cupertino}/src` was flattened into leaf
   paths, from `use` trees (nested braces expanded, `as` renames dropped) and from inline paths,
   after removing comments and each file's trailing `#[cfg(test)] mod` block;
2. that gave 167 paths for Material, 92 for Cupertino and 195 in their union, from nine crates;
3. each leaf was rewritten to its SDK path, with an associated item reduced to its type.

| Crate | Leaf paths | SDK path |
|---|---|---|
| `flui_widgets` | 90 | `widgets` (whole crate) |
| `flui_types` (split by owner, ADR-0098 §8) | 37 | `geometry` (the whole `flui_foundation::geometry` module); `painting::{Alignment, Clip, Paint, Path, Border, BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, Color, FontWeight, TextDirection, TextStyle}` from `flui_painting`; `platform::{Brightness, Locale}` from `flui_platform_api` |
| `flui_view` | 29 | `view` (glob of the whole crate, `__runtime` shadowed); its macros expand through `$crate::`, so they work through the re-export |
| `flui_animation` | 19 | `animation` (whole crate) |
| `flui_foundation` | 7 | `foundation` (whole crate) |
| `flui_rendering` | 6 | `rendering::{RenderUpdateImpact, BoxConstraints, HitTestBehavior, BoxProtocol}`; `pipeline::Canvas` is `flui_painting::Canvas`, so `painting::Canvas` |
| `flui_interaction` | 3 | `interaction::{DragDownDetails, FocusNode}` |
| `flui_objects` | 3 | `pipeline::{PathClipConfiguration, RenderPhysicalShape, TranslationFraction}` |
| `flui_rendering::view` | 1 | `pipeline::ScrollDirection`, read by the tab controller to tell a user scroll from a programmatic one; the facade has no path for it |
| `flui_scheduler` | 1 | `LocalPostFrameHandle`, which `flui_view` already re-exports: `view::LocalPostFrameHandle` |

`painting::DrawOp` is added for the packages' paint tests, the only place they name it.
`painting::ClipOp` is added for `flui-material`'s ink, which clips its ripple anti-aliased.
`flui-devtools`, measured the same way when it moved, adds `hooks::FrameSnapshot` and the
`foundation::observe` paths (see Consumers). Their
tests under `tests/` also use `flui-interaction`'s `testing` feature and `flui-testing` (whose
`widgets` module is the widget harness); those stay dev-dependencies of the packages and are
not SDK surface.

## Consumers

- **`flui-material`** (`packages/flui-material`) builds on this crate alone: its normal
  dependencies are `flui-sdk` and `tracing`, and every path in its `src`, doctests and tests goes
  through `flui_sdk::`. The port needed no new item. `TextFormField` added four paths inside
  the whole `widgets` re-export, so no new line in `src/lib.rs`: `AutovalidateMode`,
  `FormFieldHandle` and the `__private` seams `TextFormFieldConfig` and `TextFormFieldCore`,
  which the `measured` module names.
- **`flui-cupertino`** (`packages/flui-cupertino`) builds on this crate alone the same way: its
  normal dependencies are `flui-sdk` and `tracing`. Its animation owners and
  observation interfaces retain the same types through `animation`, including
  `animation::prelude`; tests reach `RenderId` as `foundation::RenderId`.
- **`flui-devtools`** (`packages/flui-devtools`) builds on this crate alone: its normal
  dependencies are `flui-sdk` plus `parking_lot`, `serde`, `serde_json`, `tracing`,
  `tracing-subscriber` and `web-time`. It needed one new item, `hooks::FrameSnapshot`, which
  its timeline turns into Chrome-trace events; the facade has no scheduler module, so the item
  sits in an Evolving module. Its inspector's ADR-0040 observation seam is
  `foundation::observe` (`TreeObserver`, the four event types) and `foundation::RebuildReason`,
  already inside the whole `foundation` re-export at the facade's path, so the `measured`
  module names them and `src/lib.rs` gains no line for them.

The kind rule of `cargo xtask workspace` (ADR-0081 §3, ADR-0088 §2) keeps the three
manifests on the SDK: an official package's normal and build dependencies are `flui-sdk` and
the contract crates, and its self-test plants each way of leaving it.

An item a package needs that is not here is added by ADR-0088 §4 (at the facade's path when the
facade has one, otherwise in an Evolving module: `pipeline` for render internals, `hooks` for
development hooks) with a line in `tests/surface.rs`'s pinned list.

## The derives resolve through the SDK

`flui-macros` looks up `flui-sdk` in the consumer's manifest before the owning crate and the
facade, and expands to `::flui_sdk::{view,foundation,animation}`, so `#[derive(StatelessView)]`
and the others work in a package that names no internal crate. The SDK comes first because a
package may carry the facade or an internal crate as a dev-dependency, which `proc-macro-crate`
does not tell from a normal one. The one shape this order cannot serve, an owner crate as a
normal dependency beside `flui-sdk` as a dev-dependency only, has no instance
(`crates/flui-macros/ARCHITECTURE.md`, "Resolve runtime paths"). The `Diagnosticable` derive
itself has no SDK path; its expansion still resolves through the SDK.

Whenever a build enables `flui/material` or `flui/cupertino` (the test scope and `--all-features`
do), `flui-sdk`'s dev-dependency on the facade reaches that catalog, and a unit test inside this
crate would see a second copy of `flui_sdk` (the one the catalog links). The surface test is an integration test
and is not affected.

The count is from source, not from rustdoc JSON; the rustdoc measurement ADR-0088 §4 asks for
needs the nightly JSON tooling and replaces this table when it lands.

## Mapping decisions

### One SDK crate with its own version

Cargo resolves each crate separately, so FLUI gives packages one crate with its own version
instead of the internal crates. The reasons are ADR-0088; the tests above pin it.


`interaction::GestureEndReason` exposes the same terminal-reason enum as the
facade (ADR-0112). Material drawer callbacks use it to settle cancelled drags
without release velocity. The existing surface list and type-identity tests
include this package-author decision.

ADR-0124 adds controlled `Slider` and `Disclosure`, their state types,
`ExpansionState`, and checked `NumericRange` configuration through the existing
whole-widget-module export. `surface` names these items as representative
package-author surface; no new framework dependency is required by a package.

ADR-0125 exposes `animation::VsyncRegistrationError` through the existing whole
animation-module export. The surface list pins this typed admission refusal;
package lifecycle owners borrow registration tokens when removing work.

### The custom-recognizer extension surface

The input-completeness spec places the recognizer extension surface here
(its design §0.2, requirements E1–E2): `interaction` re-exports the
recognizer and arena-member traits, `ArenaMembership`, `PrimaryContact` and
its contact types, the arena entry and disposition, `GestureComposition`,
`GestureBranches`, `SequencedBranch`, `CompositionError`, `MotionRole`,
`PanClaim`, `PointerKindSet` (which `GestureRecognizer::allowed_devices`
returns, so it sits here and not only behind `unstable-input`),
`GestureSettingsProvider`, `cancel_all`, the pointer event and dispatch types
and `EventTime`; `RawGestureDetector`, `GestureRecognizerFactory` and
`RecognizerContext` come through the whole `widgets` export. The facade's
`interaction` module holds every one of them, observer items included, so
the facade-path invariant holds. `tests/surface/custom_recognizer.rs`, a
module of the surface test, is the proof: a recognizer written against
`flui_sdk` alone, mounted by `RawGestureDetector`, that keeps its contact
through a parent rebuild (`a_rebuild_mid_gesture_keeps_the_drag`).

### Gesture alternatives

A package's behavioural widget takes a `GestureAlternatives` (ADR-0216):
`GestureAlternatives`, `Alternative`, `ControlSpec`, `ControlPlacement`,
`MenuSpec`, `MenuPresenter`, `MenuRequest`, `MenuItem`, `MenuCause`,
`CompanionSpec`, `CompanionLink`, `CompanionRegistration`, `CompanionGap`,
`GestureCommand`, `CommandHandle`, `GestureAlternativesError` and
`ShowContextMenuIntent` come through the whole `widgets` export, with
`SingleActivator` for a command's keys and `WidgetsLocalizations` for the
labels the alternatives make themselves. A widget whose gestures perform
several commands reaches each through `GestureCommand::handle` and checks a
command's intent with `GestureCommand::performs`, as `SwipeActions` and
`RefreshIndicator` do; a widget that drags along an axis leaves the window's
system-gesture strip to the operating system with
`ContactAdmission::outside_system_edges`, through `GestureDetector`,
`RawGestureDetector` or `AnchoredDrag::admission`, as the swiped rows do.
A widget whose commands another widget already draws as controls declares
`Alternative::Companion` over a `CompanionLink` that the owner of the shared state
makes; the companion registers on it with `CompanionLink::register` and names the node
that performs each command by a `SemanticsAnchor`, as `flui-material`'s `TabBar` does for
`TabBarView` (`gesture_alternatives::tab_bar_view_with_its_tab_bar`).
`tests/surface/behavioural_widget.rs` is the proof: a fling-to-file widget
written against `flui_sdk` alone, whose left fling performs its first command
through `GestureAlternatives::handle` and whose right fling performs its
second through `GestureCommand::handle`, passes
`flui_testing::assert_gesture_alternatives`
(`a_package_gesture_widget_reaches_its_intent_on_every_path`), performs the
second command from its gesture and leaves an edge-strip touch alone
(`a_package_gesture_widget_performs_its_second_command_from_its_gesture`).

`InteractiveViewer`'s transform seams stay crate-private:
`TransformSlot::start_mode` (a one-contact pan that becomes a scale) and
`TransformSlot::native_admission` (refusing a native pinch the viewer would
not act on). A package zoomable
viewer can mount a `TransformSlot` and receives native trackpad pinch through
it, but cannot yet start the scale from one contact or refuse a native pinch
before it is claimed; exposing them needs `ScaleStartMode` and the pan-zoom
event types on this surface.
`tests/ui/gesture_command_needs_an_intent.rs` pins the diagnostic a closure
passed where the gesture's intent belongs gets.
