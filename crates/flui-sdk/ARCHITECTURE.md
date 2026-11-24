# flui-sdk Architecture

The package-author surface of
[ADR-0088](../../docs/adr/ADR-0088-official-packages-sdk-and-facade.md) §4: one crate that an
official or third-party package depends on instead of the internal crates, so the core's crate
topology is not part of any package manifest. The facade is what an application names; the SDK
is what a package names, and it holds more of the framework's building blocks than the facade
does.

## Invariants

- **Host-free.** The normal dependency closure names none of `flui-app`, `flui-engine`, `wgpu`
  or the platform backends: tier K's forbid set in the root `[workspace.metadata.flui.reach]`,
  over this crate's default and all-features builds.
- **Same items, not copies.** Every path is a `pub use` of the item itself: a whole crate
  (`pub use flui_widgets as widgets`) or one item (`pub use flui_painting::Canvas`). No wrapper,
  newtype or re-declared trait, so `flui_sdk::m::T` and `flui::m::T` are one type wherever the
  facade has that path. `the_re_exports_are_the_facades_types` in `tests/surface.rs` fails to
  build if a curated item or a whole-module representative stops being the facade's type.
- **Paths follow the facade.** An item the facade exposes sits at the facade's path in the SDK;
  that test compares the same path on both sides. An item the facade does not expose sits in a
  module or submodule the facade does not have (`pipeline`, `hooks`, `interaction::text_input`),
  or inside a whole-crate module (`foundation`, `animation`, `view`, `widgets`) whose shared
  items keep the facade's paths. The facade's own `foundation` is curated, so the SDK's whole
  `foundation` holds more than the facade's does.
- **The composition-root seam is private.** The SDK's `view` module shadows
  `flui_view::__runtime`, and its `animation` module shadows `flui_animation::__runtime`.
  `trybuild_ui::ui_tests` in `tests/compiler_guards.rs` checks the exact E0603 diagnostic
  (`tests/ui/sdk_runtime_is_private.rs`) when a package imports the view seam, and a passing
  fixture (`tests/ui/composition_root_runtime.rs`) names the actual runtime trait through
  `flui-view` together with the public SDK view trait. An unrelated unresolved import cannot
  satisfy that snapshot.
- **The list is pinned.** `the_public_surface_is_the_measured_list` in `tests/surface.rs`
  compares every `pub` line of `src/lib.rs` with a pinned list, so adding an item is a visible
  decision, and the test's `measured` module names the items through their SDK paths, so
  removing one fails to build.
- **Evolving surface under the ceiling.** `pipeline` and `hooks` are Evolving. ADR-0088 §4
  revisits the crate when its Evolving surface beyond the hooks passes about thirty items; today
  that is the one item of `pipeline`. `hooks` holds development hooks (the plugin ABI hot reload
  mounts) and sits outside the count.
- **Own version.** `version = "0.N"`, not the workspace's: an evolving crate neither inherits
  the version nor leaves major 0.
- **One train per graph.** The guard is not here: `flui-foundation` declares
  `links = "flui_train"`, because the facade does not depend on this crate and a guard here
  could not separate an application on one train from a package on another.
  `flui-foundation` is in this crate's build and in the facade's.
- **The facade is a dev edge only.** `flui` is a path-only dev-dependency for the identity test;
  `cargo package` drops it, and nothing in `src/` names the facade.

## How an item enters

An item is in the SDK because a package needs it, or because an accepted ADR or spec places it in
the package-author surface (ADR-0088 §4). It sits at the facade's path when the facade has one,
and otherwise in an Evolving module: `pipeline` for render internals, `hooks` for development
hooks. Its line goes into the pinned list in `tests/surface.rs`, and the list's `measured`
module names it, in the same change. A whole-crate module needs no line for a new item of that
crate.

## Module map

- **Whole modules.** `foundation`, `animation`, `widgets` and `geometry` are the internal crates
  (`geometry` is `flui_foundation::geometry`) themselves. `view` and `animation` are globs of
  their crates that shadow the `__runtime` seam. Values the widgets take from other crates
  (observation in `foundation::observe`, `RebuildReason`, the typed `Inspect` model of
  ADR-0242) arrive through them.
- **Curated modules**, at the facade's paths, each item the same type as the facade's:
  - `painting`: the `paint`, `styling` and `typography` modules whole, and the root items
    `Alignment`, `AlignmentDirectional`, `Canvas`, `DrawOp` (packages' paint tests),
    `BlurSigma`, `TextSizing`, `CustomPainter` with the `CustomPaintContext` its `paint` takes,
    and the semantics regions that `CustomPainter::semantics_regions` returns (ADR-0223); the
    shadow and decoration types come
    in through `styling` and `typography` (ADR-0309, ADR-0310).
  - `rendering` and `rendering::objects`: constraints, protocol, hit-test behaviour, layout
    delegates and the configuration types widgets take from the built-in render objects.
  - `semantics`: the roles, states and ranges a widget describes itself with.
  - `scheduler`: the handles a package holds (`AsyncDriver`, `FrameWaker`, `PostFrameHandle`,
    `TaskToken`).
  - `platform`: values (locale, brightness, target platform, system preferences with every type
    its accessors return, clipboard answers, window state and host notices); `platform::text_store`
    (the text-input options a text field shares with the input method); `platform::capability`,
    the capability seam through which a package declares, provides and registers a platform
    capability (ADR-0152); and, under `unstable-input`, `platform::input_capabilities` and
    `platform::haptics` (ADR-0213, ADR-0210).
  - `interaction`: gesture details and focus, the per-kind gesture presets, arena observation,
    the custom-recognizer extension surface, the clipboard and primary-selection handles, and
    `interaction::drag`, drag and drop (ADR-0211).
- **Evolving modules**, absent from the facade:
  - `pipeline`: render internals (`ScrollDirection`).
  - `hooks`: the plugin ABI of `flui-hot-reload`'s dlopen path (ADR-0311 §6), which leaves with
    that path (ADR-0311 §7).
  - `interaction::text_input`: what a text widget's store is built from, the lock arbiter, the
    composition ledger, `committed_text` and the edit count (ADR-0151 §6).

A widget's value types sit where the facade places them, not beside the widget: `widgets` holds
only its own items, so semantics roles are in `semantics`, layout enums in `rendering::objects`,
pointer events and `GestureEndReason` (ADR-0112) in `interaction`, and
`SliverPersistentHeaderDelegate` in `view::element`.

## Consumers

- **`flui-material`** and **`flui-cupertino`** (`packages/`) build on this crate alone: their
  normal dependencies are `flui-sdk` and `tracing`, and every framework path in their `src`,
  doctests and tests goes through `flui_sdk::`. Material takes the whole `widgets`, `view` and
  `animation` surfaces, the three `painting` modules, `rendering::objects` types for its
  shapes, `painting::Alignment` and `AlignmentDirectional` for its `DataTable`, `painting::paint::ClipOp`
  for its ink, `platform::TargetPlatform` for its page-transition theme (ADR-0221),
  `foundation::Subscription` through `FocusNode::subscribe` for its text field's focus observer (ADR-0165), and
  `pipeline::ScrollDirection` for its tab controller. Cupertino takes `animation::MotionPolicy`
  (ADR-0195) through `view::environment::MediaQuery::motion_of`.
- **`flui-devtools`** (`packages/flui-devtools`) builds on this crate alone: its normal
  dependencies are `flui-sdk` plus `parking_lot`, `serde`, `serde_json`, `tracing`,
  `tracing-subscriber` and `web-time`. Its capture reads the frame spans and frame telemetry the
  framework traces, its ids are `foundation`'s, and its inspector's observation seam (ADR-0040) is
  `foundation::observe` and `foundation::RebuildReason`.
- **`flui-hot-reload`** (`packages/flui-hot-reload`) builds on this crate alone (ADR-0311 §6):
  its normal dependencies are `flui-sdk`, `tracing` and, behind `app-plugin`, `parking_lot`,
  plus the target crates of its dlopen loader. A plugin image mounts its view tree on
  `hooks::PluginPipeline` and returns a `hooks::Scene`; it measures text through a
  `hooks::TextContextHandle` of its own.

The kind rule (ADR-0081 §3, ADR-0088 §2) keeps the four manifests on the SDK: an official
package's normal and build dependencies are `flui-sdk` and the contract crates. The packages'
tests also use `flui-interaction`'s `testing` feature and `flui-testing`; those stay
dev-dependencies and are not SDK surface.

Part of the surface has no consumer in the repository: the capability seam, drag and drop, the
clipboard and primary-selection handles, the recognizer extension surface, the semantics
enums and the system preferences are placed by the ADRs named above for third-party packages.

## The derives resolve through the SDK

`flui-macros` looks up `flui-sdk` in the consumer's manifest before the owning crate and the
facade, and expands to `::flui_sdk::{view,foundation,animation}`, so `#[derive(StatelessView)]`
and the others work in a package that names no internal crate. The SDK comes first because a
package may carry the facade or an internal crate as a dev-dependency, which `proc-macro-crate`
does not tell from a normal one. The one shape this order cannot serve, an owner crate as a
normal dependency beside `flui-sdk` as a dev-dependency only, has no instance
(`crates/flui-macros/ARCHITECTURE.md`, "Resolve runtime paths"). The `#[derive(Inspect)]` derive
(ADR-0242) sits beside its trait as `flui_sdk::foundation::inspect::Inspect`;
`tests/ui/inspect_through_sdk.rs`, a case of `trybuild_ui::ui_tests`, derives and reads it through
that path.

Whenever a build enables `flui/material` or `flui/cupertino` (the test scope and `--all-features`
do), `flui-sdk`'s dev-dependency on the facade reaches that catalog, and a unit test inside this
crate would see a second copy of `flui_sdk` (the one the catalog links). The surface test is an
integration test and is not affected.

## The custom-recognizer extension surface

`interaction` re-exports what a third-party recognizer is written against: the recognizer and
arena-member traits, `ArenaMembership`, `PrimaryContact` and its contact types, the arena entry
and disposition, `GestureComposition`, `GestureBranches`, `SequencedBranch`, `CompositionError`,
`MotionRole`, `PanClaim`, `PointerKindSet` (which `GestureRecognizer::allowed_devices` returns),
`GestureSettingsProvider`, `cancel_all`, the
pointer event and dispatch types and `EventTime`; `RawGestureDetector`,
`GestureRecognizerFactory` and `RecognizerContext` come through the whole `widgets` export. The
facade's `interaction` module holds every one of them, so the paths rule holds.
`tests/surface/custom_recognizer.rs` is the proof: a recognizer written against `flui_sdk` alone,
mounted by `RawGestureDetector`, keeps its contact through a parent rebuild
(`a_rebuild_mid_gesture_keeps_the_drag`).

## Gesture alternatives

A package's behavioural widget takes a `GestureAlternatives` (ADR-0216). The types it names
(`GestureAlternatives`, `Alternative`, `ControlSpec`, `MenuSpec`, `CompanionLink`,
`GestureCommand`, `ShowContextMenuIntent` and the rest of that family) come through the whole
`widgets` export, with `SingleActivator` for a command's keys and `WidgetsLocalizations` for the
labels the alternatives make themselves. A widget whose gestures perform several commands reaches
each through `GestureCommand::handle` and checks a command's intent with
`GestureCommand::performs`; a widget that drags along an axis leaves the window's system-gesture
strip to the operating system with `ContactAdmission::outside_system_edges`. A widget whose
commands another widget already draws as controls declares `Alternative::Companion` over a
`CompanionLink`, which the owner of the shared state makes, as `flui-material`'s `TabBar` does for
`TabBarView` (`tab_bar_view_with_its_tab_bar` in `packages/flui-material/tests/gesture_alternatives.rs`).
`tests/surface/behavioural_widget.rs` is the proof: a fling-to-file widget written against
`flui_sdk` alone passes `flui_testing::gesture_alternatives::assert_gesture_alternatives`
(`a_package_gesture_widget_reaches_its_intent_on_every_path`) and performs its second command from
its gesture while leaving an edge-strip touch alone
(`a_package_gesture_widget_performs_its_second_command_from_its_gesture`).
`tests/ui/gesture_command_needs_an_intent.rs` pins the diagnostic a closure passed where the
gesture's intent belongs gets.

`InteractiveViewer`'s transform seams stay crate-private: `TransformSlot::start_mode` (a
one-contact pan that becomes a scale) and `TransformSlot::native_admission` (refusing a native
pinch the viewer would not act on). A package's zoomable viewer can mount a `TransformSlot` and
receives native trackpad pinch through it, but cannot start the scale from one contact or refuse a
native pinch before it is claimed; exposing them needs `ScaleStartMode` and the pan-zoom event
types on this surface.
