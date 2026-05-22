# Flutter ↔ FLUI Gap Matrix

**Date:** 2026-05-22
**Author:** Flutter framework architect (consultant)
**Purpose:** Definitive subsystem-level gap matrix mapping all 12 Flutter framework packages onto FLUI's 21-crate workspace, to seed the master Flutter→Rust port ROADMAP.

---

## Intro

FLUI ports Flutter's three-tree architecture (View→Element→Render) into Rust — behavior loyal, structure Rust-native (per `STRATEGY.md`). The render machine (build→layout→paint→composite) is largely built. The user-facing widget/Material/Cupertino layer is essentially absent. This document enumerates every Flutter subsystem, maps it to its FLUI home, and rates port complexity, so the roadmap is dependency-ordered and sized against real numbers.

### Methodology

- **Dart LOC** is `find <pkg> -name '*.dart' | xargs wc -l` against `.flutter/flutter-master/packages/flutter/lib/src/<pkg>/`. Includes generated `.g.dart` files (icons, keyboard maps, animated-icon data) — flagged inline where they distort the picture, since those are data tables, not logic to port.
- **FLUI status** was verified by reading each crate's `src/` file tree and spot-reading source — not inferred from crate names. `flui-rendering/src/objects/` was confirmed to hold only 7 box render objects; `flui-view` was confirmed to hold the Widget/Element framework but no widget catalog; etc.
- **Status legend:** ✅ built (subsystem substantially present and active) · 🟡 partial (some structure exists, significant gaps) · ⏸️ disabled (source exists, commented out of `[workspace.members]`) · ❌ missing (no FLUI code at all).
- **Port complexity:** **S** (≤1 week, mechanical) · **M** (1–3 weeks) · **L** (3–8 weeks) · **XL** (8+ weeks / multi-subsystem).
- Complexity ratings assume the Rust foundations FLUI already has (Slab arena, arity system, `flui-types` geometry, wgpu engine). They rate the *port*, not a from-scratch build.

### FLUI workspace state (verified against `Cargo.toml`)

- **Active (15):** flui-types, flui-foundation, flui-tree, flui-platform, flui-painting, flui-semantics, flui-scheduler, flui-layer, flui-interaction, flui-engine, flui-log, flui-hot-reload, flui-rendering, flui-view, flui-app.
- **Disabled (6):** flui-animation, flui-reactivity, flui-assets, flui-devtools, flui-cli, flui-build (source exists, commented out).
- **Missing crates (no source):** flui-widgets, flui-material, flui-cupertino, flui-gestures-catalog (no equivalent), flui-localizations.

---

## 1. `foundation` — 11,420 Dart LOC

Base utilities: diagnostics, change-notification, key types, platform constants, bindings scaffold.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `diagnostics.dart` — DiagnosticableTree, property nodes, tree dump | 3707 | `flui-foundation/debug.rs`, `flui-rendering` Diagnosticable | 🟡 | L | Foundation has a `debug.rs`; full `DiagnosticPropertiesBuilder` / `DiagnosticsNode` tree-dump surface needed by devtools/inspector is not ported. Inspector blocker. |
| `change_notifier.dart` — ChangeNotifier, ValueNotifier, Listenable | 568 | `flui-foundation/notifier.rs` | ✅ | — | Ported. |
| `assertions.dart` — FlutterError, error reporting, stack demangling | 1304 | `flui-foundation/assert.rs`, `flui-view/view/error.rs` (`FlutterError`) | 🟡 | M | `FlutterError` exists; full error-widget + assertion-formatting surface partial. |
| `binding.dart` — BindingBase, service-extension registration | 992 | `flui-foundation/binding.rs`, `flui-app/bindings/` | 🟡 | M | Binding scaffold exists; service-extension registry (devtools VM-service hooks) absent. |
| `key.dart` / `unique_widget.dart` keys | 116 | `flui-foundation/key.rs`, `flui-view/key/` | ✅ | — | LocalKey/ValueKey/ObjectKey/GlobalKey all present (`flui-view/key/`). |
| `basic_types.dart` / `collections.dart` / `object.dart` | 642 | `flui-foundation`, `flui-types` | ✅ | S | Covered by Rust stdlib + `flui-types`. |
| `persistent_hash_map.dart` — InheritedWidget lookup map | 416 | `flui-view` inherited access | 🟡 | M | FLUI uses `TypeId` registry for InheritedView (per STRATEGY); persistent-map structure differs by design. |
| `observer_list.dart` / `node.dart` | 320 | `flui-foundation`, `flui-tree` | ✅ | S | `flui-tree` covers node abstraction. |
| `licenses.dart` — license registry | 337 | — | ❌ | S | Not ported; minor (license-page support). |
| `isolates.dart` / `_isolates_*.dart` — compute() | 104 | `flui-platform/executor.rs` | 🟡 | S | Rust uses thread pool / `background_executor`; `compute()` shape is a thin wrapper to add. |
| `memory_allocations.dart` / `timeline.dart` — profiling hooks | 752 | `flui-log`, `tracing` | 🟢 replace | S | `tracing` covers timeline; legitimate Rust-native replacement. |
| `platform.dart` / `_platform_*.dart` — defaultTargetPlatform | 113 | `flui-types/platform/target_platform.rs` | ✅ | — | Ported. |
| `bitfield.dart` / `stack_frame.dart` / `serialization.dart` / `synchronous_future.dart` / `print.dart` / `unicode.dart` / `constants.dart` / `capabilities.dart` | ~1240 | scattered / stdlib | 🟢 mostly | S | Most are Dart-runtime workarounds Rust does not need. |

**Coverage estimate: ~55%.** Diagnostics tree-dump is the one real gap (inspector dependency).

---

## 2. `gestures` — 14,330 Dart LOC

Pointer event model, hit-testing core, gesture-recognizer arena.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `events.dart` — PointerEvent hierarchy | 2606 | `flui-interaction/events.rs`, `flui-types/gestures/pointer.rs` | ✅ | — | Ported; FLUI uses W3C `ui-events` crate. |
| `recognizer.dart` — GestureRecognizer base, arena member | 876 | `flui-interaction/recognizers/recognizer.rs` | ✅ | — | Ported. |
| `tap.dart` / `tap_and_drag.dart` / `multitap.dart` | 3350 | `flui-interaction/recognizers/tap.rs`, `double_tap.rs`, `multi_tap.rs` | ✅ | — | Ported. |
| `monodrag.dart` / `multidrag.dart` / `drag*.dart` / `eager.dart` | 2557 | `flui-interaction/recognizers/drag.rs` | ✅ | — | Ported. |
| `long_press.dart` | 882 | `flui-interaction/recognizers/long_press.rs` | ✅ | — | Ported. |
| `scale.dart` | 860 | `flui-interaction/recognizers/scale.rs` | ✅ | — | Ported. |
| `force_press.dart` | 372 | `flui-interaction/recognizers/force_press.rs` | ✅ | — | Ported. |
| `arena.dart` — GestureArenaManager | 304 | `flui-interaction/arena.rs` | ✅ | — | Ported. |
| `hit_test.dart` — HitTestResult, HitTestable | 307 | `flui-interaction/routing/hit_test.rs`, `flui-rendering/hit_testing/` | ✅ | — | Ported. |
| `binding.dart` — GestureBinding | 649 | `flui-interaction/binding.rs` | ✅ | — | Ported. |
| `velocity_tracker.dart` / `lsq_solver.dart` | 673 | `flui-interaction/processing/velocity.rs`, `flui-types/gestures/velocity.rs` | ✅ | — | Ported. |
| `pointer_router.dart` / `pointer_signal_resolver.dart` | 281 | `flui-interaction/routing/pointer_router.rs`, `signal_resolver.rs` | ✅ | — | Ported. |
| `converter.dart` / `resampler.dart` | 675 | `flui-interaction/processing/raw_input.rs`, `resampler.rs` | ✅ | — | Ported. |
| `team.dart` / `monodrag` team | 163 | `flui-interaction/team.rs` | ✅ | — | Ported. |

**Coverage estimate: ~95%.** `flui-interaction` is one of the most complete ports in the workspace. This is the model for what the rest should reach.

---

## 3. `animation` — 5,283 Dart LOC

Curves, tweens, animation controllers, status listeners.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `curves.dart` — Curve catalog, Cubic, Curves.* | 1895 | `flui-animation/curve.rs` (985 LOC) | ⏸️ | M | Source exists, crate disabled. Re-enable + verify catalog parity. |
| `animation_controller.dart` — AnimationController, vsync drive | 1054 | `flui-animation/controller.rs` (1079 LOC) | ⏸️ | M | Source exists, disabled. Needs `flui-scheduler` Ticker wiring re-verified. |
| `animations.dart` — ProxyAnimation, ReverseAnimation, etc. | 740 | `flui-animation/proxy.rs`, `reverse.rs`, `compound.rs` | ⏸️ | S | Source exists, disabled. |
| `tween.dart` / `tween_sequence.dart` | 738 | `flui-animation/tween.rs`, `tween_types.rs` (1187 LOC) | ⏸️ | M | Source exists, disabled. |
| `animation.dart` — Animation<T> base, AnimationStatus | 412 | `flui-animation/animation.rs`, `status.rs` | ⏸️ | S | Source exists, disabled. |
| `listener_helpers.dart` — AnimationLocalListenersMixin | 268 | `flui-animation/ext.rs` | ⏸️ | S | Source exists, disabled. |
| `animation_style.dart` — AnimationStyle | 176 | — | ❌ | S | Small; not in disabled crate. Add on re-enable. |

**Coverage estimate: ~85% (but ⏸️ disabled — counts as 0% effective until re-enabled).** `flui-animation` is 7,475 Rust LOC — *larger* than the 5,283 Dart source, suggesting a fairly thorough port. Primary work is re-enable + integration repair, not new authoring.

---

## 4. `physics` — 893 Dart LOC

Simulation primitives for fling/scroll/spring.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `spring_simulation.dart` | 397 | `flui-types/physics/spring.rs` (353 LOC) | ✅ | — | Ported. |
| `friction_simulation.dart` | 201 | `flui-types/physics/friction.rs` (239 LOC) | ✅ | — | Ported. |
| `gravity_simulation.dart` | 95 | `flui-types/physics/gravity.rs` (161 LOC) | ✅ | — | Ported. |
| `clamped_simulation.dart` | 70 | `flui-types/physics/mod.rs` | ✅ | — | Ported. |
| `simulation.dart` — Simulation base | 60 | `flui-types/physics/mod.rs` | ✅ | — | Ported. |
| `tolerance.dart` | 49 | `flui-types/physics/tolerance.rs` (115 LOC) | ✅ | — | Ported. |
| `utils.dart` | 21 | `flui-types/physics/mod.rs` | ✅ | — | Ported. |

**Coverage estimate: ~100%.** The whole package lives in `flui-types/src/physics/` (1,000 Rust LOC). Done. **Note:** `physics` arguably did not warrant its own Flutter package and FLUI folded it into `flui-types` — a sound consolidation, no `flui-physics` crate needed.

---

## 5. `scheduler` — 2,192 Dart LOC

Frame scheduling, vsync ticker, frame phases, task priority queue.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `binding.dart` — SchedulerBinding, frame phases, transient/persistent callbacks, post-frame | 1470 | `flui-scheduler/scheduler.rs`, `frame.rs`, `vsync.rs` | ✅ | — | Ported (7,902 Rust LOC in crate). |
| `ticker.dart` — Ticker, TickerProvider | 554 | `flui-scheduler/ticker.rs` | ✅ | — | Ported. |
| `priority.dart` — Priority enum, scheduler task priority | 54 | `flui-scheduler/task.rs`, `budget.rs` | ✅ | — | Ported. |
| `debug.dart` / `service_extensions.dart` | 114 | `flui-scheduler` / `tracing` | 🟢 replace | S | Service-extension hook minor. |

**Coverage estimate: ~95%.** `flui-scheduler` is well ported; per `docs/research/2026-05-21-flui-scheduler-audit-draft.md` it has had a dedicated audit.

---

## 6. `painting` — 24,890 Dart LOC

Painting primitives: text painter, decorations, borders, gradients, image providers/cache, edge insets, alignment.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `text_painter.dart` — TextPainter, line layout, caret metrics | 1841 | `flui-painting/text_painter/` (751 LOC), `text_layout/` (1031 LOC) | 🟡 | L | Core measure/layout/paint present; full caret/selection/affinity metrics surface partial. cosmic-text backs it. |
| `text_style.dart` / `strut_style.dart` / `text_scaler.dart` | 2626 | `flui-types/typography/text_style.rs` (382 LOC), `text_scaler.rs` | 🟡 | M | `TextStyle` exists but smaller than Dart; strut-style + full inheritance resolution needs filling. |
| `text_span.dart` / `inline_span.dart` / `placeholder_span.dart` | 1127 | `flui-types/typography/text_spans.rs` | 🟡 | M | Span tree present; placeholder/inline-widget span partial. |
| `image_provider.dart` / `image_stream.dart` / `image_cache.dart` / `image_resolution.dart` / `decoration_image.dart` / `_network_image_*` | 5358 | `flui-assets` (⏸️), `flui-painting/binding.rs`, `flui-types/painting/image.rs` | 🟡⏸️ | L | Image cache lock noted in `flui-painting/binding.rs`; `flui-assets` (disabled) holds image loading. ImageProvider resolution chain partial. |
| `gradient.dart` — Linear/Radial/Sweep gradients | 1179 | `flui-types/styling/gradient.rs` (493 LOC) | 🟡 | M | Gradient types present, smaller than Dart; sweep/transform variants need check. |
| `box_border.dart` / `borders.dart` / `border_radius.dart` | 3046 | `flui-types/styling/border.rs`, `box_border.rs`, `border_radius.rs` | 🟡 | M | Box border + radius present; full ShapeBorder hierarchy below. |
| `box_decoration.dart` / `decoration.dart` / `shape_decoration.dart` | 1329 | `flui-types/styling/decoration.rs` (331 LOC) | 🟡 | M | `BoxDecoration` present; `ShapeDecoration` partial. |
| ShapeBorder catalog: `rounded_rectangle_border.dart`, `stadium_border.dart`, `circle_border.dart`, `beveled_*`, `continuous_*`, `oval_border.dart`, `star_border.dart`, `linear_border.dart` | ~3500 | `flui-types/styling/` | 🟡 | L | Most ShapeBorder variants (stadium/star/beveled/continuous/oval/linear) not ported. Needed by Material/Cupertino. |
| `edge_insets.dart` — EdgeInsets, EdgeInsetsDirectional | 1075 | `flui-types/layout/edges.rs` | ✅ | — | Ported. |
| `alignment.dart` / `fractional_offset.dart` | 952 | `flui-types/layout/alignment.rs`, `fractional_offset.rs`, `painting/alignment.rs` | ✅ | — | Ported. |
| `box_shadow.dart` / `box_fit.dart` / `clip.dart` | 506 | `flui-types/styling/shadow.rs`, `layout/box.rs`, `painting/clipping.rs` | ✅ | — | Ported. |
| `colors.dart` — Color, HSL/HSV | 515 | `flui-types/styling/color.rs`, `color32.rs`, `hsl_hsv.rs` | ✅ | — | Ported. |
| `matrix_utils.dart` | 652 | `flui-types/geometry/matrix4.rs`, `transform.rs` | ✅ | — | Ported. |
| `basic_types.dart` / `geometry.dart` / `paint_utilities.dart` / `debug.dart` / `shader_warm_up.dart` / `flutter_logo.dart` / `notched_shapes.dart` / `image_decoder.dart` | ~1500 | `flui-types/painting/`, `flui-engine` | 🟡 | M | Mixed; `flutter_logo` cosmetic, `notched_shapes` needed for Material BottomAppBar. |

**Coverage estimate: ~55%.** Geometry/color/edge-insets/alignment are solid in `flui-types`. The gaps: full ShapeBorder catalog, ImageProvider chain (entangled with disabled `flui-assets`), strut-style + placeholder spans, decoration completeness. These are Material/Cupertino prerequisites.

---

## 7. `semantics` — 7,865 Dart LOC

Accessibility tree, semantics nodes/actions/flags, semantics events.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `semantics.dart` — SemanticsNode, SemanticsConfiguration, SemanticsOwner, SemanticsProperties, merge logic | 7232 | `flui-semantics/node.rs`, `configuration.rs`, `owner.rs`, `properties.rs`, `tree.rs`, `update.rs` | 🟡 | L | `flui-semantics` (6,619 Rust LOC) ports the node/owner/config/tree structure. Per `docs/research/2026-05-22-flui-layer-semantics-audit.md` it has gaps; the platform-bridge half (AT-SPI / UIA / a11y APIs) is not wired. |
| `binding.dart` — SemanticsBinding | 279 | `flui-semantics/binding.rs` | 🟡 | M | Binding shell present. |
| `semantics_event.dart` — AnnounceSemanticsEvent etc. | 238 | `flui-semantics/event.rs` | ✅ | — | Ported. |
| `semantics_service.dart` — SemanticsService.announce | 104 | `flui-semantics` | 🟡 | S | Service shell; needs platform announce channel. |
| `debug.dart` | 12 | `flui-semantics` | ✅ | S | Trivial. |

**Coverage estimate: ~65%.** Tree-data structure ported; the OS-accessibility-API bridge (the part that makes it actually work with screen readers) is the gap.

---

## 8. `rendering` — 52,118 Dart LOC

The render-object machine: RenderObject/RenderBox base, layout protocol, layer tree, proxy boxes, paragraph/editable, slivers/viewport, flex/stack/table.

This is the heart of the port. FLUI splits it across **`flui-rendering`** (render-object tree + protocol), **`flui-layer`** (the layer tree — Flutter keeps layers inside `rendering/layer.dart`), and **`flui-painting`/`flui-engine`** (raster).

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `object.dart` — RenderObject, PipelineOwner, ParentData, PaintingContext, layout/paint orchestration | 6806 | `flui-rendering/traits/`, `pipeline/owner.rs`, `parent_data/`, `context/` | ✅ | — | Ported. The arity system + Slab + `PipelineOwner` is the structural rewrite. Heavily audited (`docs/research/2026-05-22-flui-rendering-engine-audit.md`). |
| `box.dart` — RenderBox, BoxConstraints, intrinsic sizing, baseline | 3388 | `flui-rendering/traits/render_box.rs`, `constraints/box_constraints.rs` | ✅ | — | Ported. |
| `layer.dart` — Layer tree, ContainerLayer, all layer subtypes, Scene | 3029 | `flui-layer/layer/` (24 layer files), `tree/`, `scene.rs`, `compositor/` | ✅ | — | Ported into dedicated `flui-layer` crate (10,718 Rust LOC) — all clip/opacity/transform/shader/backdrop/leader-follower/platform-view/texture layers present. |
| `proxy_box.dart` — ~40 RenderProxyBox subtypes (Opacity, ClipRect, ClipRRect, DecoratedBox, Transform, Padding-as-proxy, BackdropFilter, ColorFiltered, CustomPaint, FittedBox, FractionalTranslation, MouseRegion, Listener, IgnorePointer, AbsorbPointer, RepaintBoundary, AnnotatedRegion, etc.) | 4819 | `flui-rendering/objects/` | 🟡 | XL | **Only `opacity.rs`, `transform.rs`, `colored_box.rs` ported.** ~37 proxy render objects missing: DecoratedBox, ClipRect/RRect/Path/Oval, BackdropFilter, ColorFiltered, ShaderMask, CustomPaint, FittedBox, MouseRegion, Listener, IgnorePointer/AbsorbPointer, RepaintBoundary, AnnotatedRegion, PhysicalModel, etc. Major gap. |
| `shifted_box.dart` — RenderPadding, RenderAlign, RenderCenter, RenderPositionedBox, RenderConstrainedBox, RenderBaseline, RenderFractionallySizedBox, RenderCustomSingleChildLayoutBox | 1629 | `flui-rendering/objects/padding.rs`, `center.rs`, `sized_box.rs` | 🟡 | L | Padding/Center/SizedBox ported; Align, FractionallySizedBox, Baseline, CustomSingleChildLayout, ConstrainedBox missing. |
| `flex.dart` — RenderFlex (Row/Column layout) | 1505 | `flui-rendering/objects/flex.rs` (461 LOC) | ✅ | — | Ported. |
| `stack.dart` — RenderStack, RenderIndexedStack | 899 | — | ❌ | M | No RenderStack. Blocks Stack widget. |
| `wrap.dart` — RenderWrap | 891 | — | ❌ | M | No RenderWrap. Blocks Wrap widget. |
| `paragraph.dart` — RenderParagraph (text layout render object) | 3673 | — (text layout lives in `flui-painting/text_layout`) | 🟡 | L | The render-object wrapper around TextPainter is absent; `flui-painting` has the layout engine but no `RenderParagraph`. Blocks Text widget. |
| `editable.dart` — RenderEditable (text-field render object) | 3156 | — | ❌ | XL | No RenderEditable. Blocks all text input (EditableText / TextField). One of the single largest missing units. |
| `viewport.dart` — RenderViewport, RenderShrinkWrappingViewport | 2261 | — (sliver protocol exists in `flui-rendering/protocol/sliver_protocol.rs`) | ❌ | XL | Protocol/constraints/geometry types exist; no actual `RenderViewport`. Blocks all scrolling. |
| `sliver.dart` + `sliver_*.dart` (list, grid, fixed-extent, multi-box-adaptor, persistent-header, padding, fill, group, tree) | ~8000 | — (sliver protocol/constraints only) | ❌ | XL | `SliverConstraints`/`SliverGeometry` ported as types; zero sliver render objects. Blocks ListView/GridView/CustomScrollView. |
| `table.dart` / `table_border.dart` — RenderTable | 1958 | `flui-rendering/parent_data/table_text.rs` (parent-data only) | ❌ | L | Parent-data shim exists; no RenderTable. |
| `custom_paint.dart` — RenderCustomPaint | 1159 | `flui-rendering/delegates/custom_painter.rs` (delegate trait only) | 🟡 | M | CustomPainter delegate trait exists; RenderCustomPaint object missing. |
| `custom_layout.dart` / `flow.dart` / `list_body.dart` | 1273 | `flui-rendering/delegates/` (delegate traits) | 🟡 | M | Delegate traits ported (`multi_child_layout_delegate.rs`, `flow_delegate.rs`); render objects missing. |
| `mouse_tracker.dart` | 431 | `flui-interaction/mouse_tracker.rs`, `flui-rendering/input/` (referenced in PORT.md) | ✅ | — | Ported. |
| `view.dart` — RenderView (render-tree root) | 578 | `flui-rendering/view/render_view.rs` | ✅ | — | Ported. |
| `binding.dart` — RendererBinding | 981 | `flui-rendering/binding/`, `flui-app/bindings/renderer_binding.rs` | ✅ | — | Ported. |
| `image.dart` — RenderImage | 476 | — | ❌ | M | No RenderImage. Blocks Image widget. |
| `animated_size.dart` / `rotated_box.dart` | 581 | — | ❌ | M | Missing. |
| `platform_view.dart` / `texture.dart` / `performance_overlay.dart` | 1101 | `flui-layer/layer/platform_view.rs`, `texture.rs`, `performance_overlay.rs` | 🟡 | M | Layer-side present; render-object side missing. |
| `selection.dart` — SelectionRegistrar, selectable render-object protocol | 922 | — | ❌ | L | No selection protocol. Blocks SelectableText/SelectableRegion. |
| `viewport_offset.dart` / `layout_helper.dart` / `tweens.dart` / `error.dart` / `debug*.dart` / `service_extensions.dart` / `image_filter_config.dart` | ~1500 | `flui-rendering/view/viewport_offset.rs`, scattered | 🟡 | M | ViewportOffset ported; misc helpers partial. |

**Coverage estimate: ~40%.** The *engine* of rendering (RenderObject base, PipelineOwner, RenderBox protocol, layer tree, flex) is solidly ported. The *render-object catalog* is ≈15% — only 7 of ~80+ concrete render objects exist. **Slivers/viewport (scrolling), RenderParagraph (text), RenderEditable (input) are the three XL holes.**

---

## 9. `widgets` — 157,402 Dart LOC

The widgets package is **two things in one**: the **framework** (Widget/Element/BuildContext/State/InheritedWidget — the three-tree machinery) and the **widget catalog** (Container, Row, Text, Image, ScrollView, GestureDetector — the actual user-facing widgets). FLUI splits these: `flui-view` is the framework half; the catalog has **no `flui-widgets` crate at all**.

### 9a. Framework half → `flui-view` (active, 14,261 Rust LOC)

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `framework.dart` — Widget, Element, BuildContext, State, StatelessWidget, StatefulWidget, InheritedWidget, ProxyWidget, ParentDataWidget, RenderObjectWidget, GlobalKey, reconciliation | 7455 | `flui-view/view/` (view.rs, stateless.rs, stateful.rs, inherited.rs, proxy.rs, render.rs, parent_data.rs), `element/`, `key/`, `owner/`, `tree/reconciliation.rs` | ✅ | — | Ported. The unified `Element<V,A,B>` reconciler (see `flui-view/UNIFIED_ELEMENT.md`) is the structural rewrite of the mixin hierarchy. Lifecycle FSM, keyed reconciliation, InheritedView via `TypeId` registry all present. |
| `binding.dart` — WidgetsBinding, WidgetsBindingObserver, runApp | 2155 | `flui-view/binding.rs` (1328 LOC) | ✅ | — | Ported. |
| `notification_listener.dart` / notifications | 177 | `flui-view/element/notification.rs` | ✅ | — | Ported (Notification system present in `lib.rs` re-exports). |
| `inherited_model.dart` / `inherited_notifier.dart` / `inherited_theme.dart` | 656 | `flui-view/view/inherited.rs` | 🟡 | M | Base InheritedView present; InheritedModel (aspect-based) + InheritedTheme partial. |
| `lookup_boundary.dart` | 359 | `flui-view/element/inherited_access.rs` | 🟡 | S | Likely partial. |
| `unique_widget.dart` / `widget_state.dart` / `widget_inspector.dart` | ~5600 | inspector → `flui-devtools` (⏸️) | ⏸️❌ | XL | `widget_inspector.dart` (4618 LOC) is the inspector backend — belongs in disabled `flui-devtools`; `widget_state.dart` (1152 LOC, WidgetStatesController) missing. |

### 9b. Widget catalog → **no `flui-widgets` crate (❌ all missing)**

Every row below is **❌ missing** unless noted. This is the single biggest greenfield block in the port.

| Dart subsystem | Dart LOC | Needs render object | Port complexity | Gap note |
| --- | --- | --- | --- | --- |
| `basic.dart` — ~80 core widgets: Padding, Center, Align, Row, Column, Flex, Stack, Wrap, SizedBox, ConstrainedBox, DecoratedBox, Opacity, Transform, ClipRect/RRect/Path/Oval, CustomPaint, RichText, RawImage, Listener, MouseRegion, IgnorePointer, AbsorbPointer, RepaintBoundary, Baseline, FractionallySizedBox, FittedBox, AspectRatio, IntrinsicWidth/Height, LimitedBox, Offstage, OverflowBox, SizedOverflowBox, Flow, Table, IndexedStack, PhysicalModel, BackdropFilter, ShaderMask, ColorFiltered, etc. | 8414 | many (see §8) | XL | The foundational widget set. Each widget is a thin `RenderObjectWidget` over a render object — but ~37 of those render objects don't exist yet (§8 proxy_box/shifted_box gap). |
| `container.dart` — Container | 494 | composite | M | The single most-used widget. Composite of Padding/DecoratedBox/ConstrainedBox/Transform. |
| `text.dart` / `widget_span.dart` / `default_text_style` | 1928 | RenderParagraph (❌) | L | Text/RichText/DefaultTextStyle. Blocked on RenderParagraph. |
| `image.dart` / `image_icon.dart` / `fade_in_image.dart` / `_web_image*` | 2767 | RenderImage (❌) | L | Image widget + providers. Blocked on RenderImage + flui-assets. |
| `icon.dart` / `icon_theme.dart` / `icon_data.dart` / `icon_theme_data.dart` | 819 | RenderParagraph-ish | M | Icon rendering (glyph from font). |
| `scroll_view.dart` / `scrollable.dart` / `scroll_position.dart` / `scroll_physics.dart` / `scroll_controller.dart` / `scroll_activity.dart` / `scroll_*` (~20 files) | ~14000 | RenderViewport + slivers (❌) | XL | Entire scrolling subsystem: Scrollable, ScrollView, ListView, GridView, CustomScrollView, ScrollController, ScrollPhysics, scroll notifications. Blocked on viewport/slivers (§8). |
| `sliver.dart` / `sliver_*.dart` (~10 files) | ~5000 | sliver render objects (❌) | XL | SliverList/Grid/Padding/Fill/PersistentHeader/etc. widget wrappers. |
| `single_child_scroll_view.dart` / `nested_scroll_view.dart` / `page_view.dart` / `list_wheel_scroll_view.dart` / `two_dimensional_*` | ~9000 | viewport variants (❌) | XL | Scroll-view variants. |
| `scrollbar.dart` / `overscroll_indicator.dart` / `stretch_effect.dart` | ~3700 | RenderProxyBox | L | Scrollbar + overscroll glow/stretch. |
| `gesture_detector.dart` / `drag_target.dart` / `interactive_viewer.dart` | 4222 | uses flui-interaction | L | GestureDetector, Draggable/DragTarget, InteractiveViewer. Recognizers exist (`flui-interaction`); the *widgets* don't. |
| `navigator.dart` / `routes.dart` / `pages.dart` / `router.dart` / `heroes.dart` / `pop_scope.dart` / `navigator_pop_handler.dart` / `will_pop_scope.dart` | ~14000 | Overlay (🟡 in flui-app) | XL | The entire navigation system: Navigator 1.0 + 2.0 (Router/RouteInformationParser), routes, pages, Hero animations. `flui-app/overlay/` has an overlay manager shell. Route-notification *binding* hooks exist in `flui-view/binding.rs`. |
| `overlay.dart` / `modal_barrier.dart` | 3387 | RenderStack-ish | L | Overlay/OverlayEntry. `flui-app/overlay/` is a partial start (🟡). |
| `focus_manager.dart` / `focus_scope.dart` / `focus_traversal.dart` | 5836 | — | XL | Focus system. `flui-interaction/routing/focus.rs` + `focus_scope.rs` exist (the *manager* primitives); the Focus/FocusScope/FocusTraversalGroup *widgets* don't. Partial. |
| `actions.dart` / `shortcuts.dart` / `default_text_editing_shortcuts.dart` / `text_editing_intents.dart` / `keyboard_listener.dart` / `raw_keyboard_listener.dart` | 6041 | — | XL | Actions & Shortcuts (intents, key bindings). Nothing ported. |
| `editable_text.dart` / `text_selection.dart` / `selectable_region.dart` / `text_editing_*` / `undo_history.dart` / `selection_container.dart` / `magnifier.dart` / `context_menu*` / `system_context_menu.dart` / `text_selection_toolbar*` / `spell_check.dart` / `default_selection_style.dart` | ~25000 | RenderEditable (❌) | XL | The text-input/selection megasubsystem. Blocked on RenderEditable + selection protocol (§8). Among the largest single blocks in the whole port. |
| `implicit_animations.dart` / `transitions.dart` / `animated_*.dart` / `dual_transition_builder.dart` / `status_transitions.dart` / `tween_animation_builder.dart` / `repeating_animation_builder.dart` / `snapshot_widget.dart` | ~9000 | uses flui-animation (⏸️) | L | AnimatedContainer, AnimatedOpacity, FadeTransition, etc., AnimatedSwitcher, AnimatedCrossFade. Blocked on `flui-animation` re-enable. |
| `media_query.dart` | 2452 | InheritedWidget | M | MediaQuery — InheritedView; needs window-metrics plumbing from `flui-platform`. |
| `layout_builder.dart` / `sliver_layout_builder.dart` / `orientation_builder.dart` | 671 | RenderObject callback | M | LayoutBuilder. `flui-rendering/delegates/custom_render_callback` design doc exists. |
| `app.dart` / `view.dart` / `_window*.dart` (win32/linux/macos/positioner) / `title.dart` | ~13000 | — | XL | WidgetsApp, View, multi-window (`_window_*.dart` — 7,500 LOC of platform-specific window widgets). FLUI handles windows in `flui-platform`; the *widget-layer* multi-window glue is missing. |
| `form.dart` / `autofill.dart` / `autocomplete.dart` | 2075 | — | L | Form/FormField/Autocomplete. |
| `table.dart` (widget) / `list_body.dart`-equiv / `grid_paper.dart` | ~700 | RenderTable (❌) | M | Table widget. |
| `reorderable_list.dart` / `dismissible.dart` / `draggable_scrollable_sheet.dart` | 3659 | various | L | Reorderable list, Dismissible, draggable sheet. |
| `transitions` for routes: `page_transitions_builder.dart` | 341 | — | M | Page transition builders. |
| `ticker_provider.dart` / `app_lifecycle_listener.dart` | 955 | flui-scheduler | S | TickerProviderStateMixin widget — thin over `flui-scheduler`. |
| `restoration.dart` / `restoration_properties.dart` / `page_storage.dart` | 1967 | — | L | State restoration. May be a legitimate partial-skip (mobile-process-death feature). |
| `localizations.dart` / `default_text_editing_shortcuts` locale bits | 971 | — | M | Localizations infrastructure. New `flui-localizations` likely. |
| `widget_state.dart` / `toggleable.dart` / `radio_group.dart` / `raw_radio.dart` | 2443 | — | M | WidgetState (hover/pressed/focused) controller + toggleable mixin — prerequisite for all Material/Cupertino interactive controls. |
| `safe_area.dart` / `display_feature_sub_screen.dart` / `annotated_region.dart` / `banner.dart` / `placeholder.dart` / `spacer.dart` / `preferred_size.dart` / `visibility`-likes / `color_filter.dart` / `image_filter.dart` / `texture.dart` / `performance_overlay.dart` / `flutter_logo.dart` / misc | ~3500 | various | M | Long tail of small utility widgets. |
| `async.dart` — FutureBuilder, StreamBuilder | 672 | — | M | Async builders. |
| `value_listenable_builder.dart` / `tween_animation_builder.dart` / `notification_listener.dart` builders | ~500 | — | S | Listenable builders. |
| `scroll_notification_observer.dart` / `size_changed_layout_notifier.dart` / `automatic_keep_alive.dart` | 868 | — | M | Scroll/layout notification helpers. |
| `widget_previews` (separate top-level dir, not counted in 157k) | — | — | — | Tooling; skip. |

**Coverage estimate: framework ~85%, catalog ~2%. Package blended ~25%.** The framework half (`flui-view`) is genuinely close. The catalog half is the largest single body of unwritten code in the entire port — roughly **110,000 Dart LOC of widgets with no FLUI home.**

---

## 10. `material` — 210,800 Dart LOC

Material Design widget catalog + theming. **Entirely missing — no `flui-material` crate.** Note: `icons.dart` (29,454 LOC) and `animated_icons/data/*.g.dart` (~38,000 LOC) are generated icon data tables — ~67k of the 210k is data, not logic. Effective logic ≈ **144,000 LOC**.

Material widgets, grouped:

| Group | Dart files | Dart LOC (approx) | FLUI home | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- | --- |
| **Theming** — theme_data, color_scheme, typography, text_theme, ~50 `*_theme.dart` files, material_state, motion, shadows, elevation_overlay | ~55 files | ~28,000 | `flui-app/theme/` (392 LOC shell) | 🟡❌ | XL | `flui-app/theme/` has a 392-LOC `ThemeData`/colors stub. Real `ThemeData` + `ColorScheme` (Material 3) + ~50 per-component theme structs unported. |
| **Buttons** — elevated, filled, text, outlined, icon_button, button_style(_button), floating_action_button(+location/theme), toggle_buttons, segmented_button, back_button, action_buttons, dropdown, expand_icon, button(_bar/_theme), material_button | ~30 files | ~18,000 | — | ❌ | XL | All Material buttons + ButtonStyle system. |
| **App structure** — scaffold, app_bar, bottom_app_bar, sliver_app_bar (in app_bar), drawer, navigation_drawer, navigation_bar, navigation_rail, bottom_navigation_bar, tabs/tab_bar/tab_controller, flexible_space_bar, app, page, banner | ~25 files | ~22,000 | — | ❌ | XL | Scaffold (3521 LOC) + AppBar (2620) + tabs (2944) + nav surfaces. Core app chrome. |
| **Input / text fields** — text_field, text_form_field, input_decorator (6107 LOC!), input_border, selectable_text, dropdown_menu(+form_field/theme), autocomplete | ~12 files | ~14,500 | — | ❌ | XL | TextField + InputDecorator. Blocked on RenderEditable (§8). |
| **Dialogs / sheets / menus** — dialog, bottom_sheet, snack_bar, menu_anchor (4265 LOC), popup_menu, menu_style/theme, menu_button_theme, menu_bar_theme, banner | ~15 files | ~12,500 | — | ❌ | XL | Dialogs, modal/persistent sheets, SnackBar, menu system. |
| **Selection controls** — checkbox(+list_tile/theme), radio(+list_tile/theme), switch(+list_tile/theme), slider(+theme/parts/value_indicator), range_slider(+parts) | ~18 files | ~20,000 | — | ❌ | XL | Checkbox/Radio/Switch/Slider/RangeSlider. Blocked on WidgetState + toggleable. |
| **Pickers** — date_picker(+theme), calendar_date_picker, input_date_picker_form_field, time_picker(+theme), date | ~7 files | ~9,400 | — | ❌ | XL | Date/time pickers. |
| **Lists / tables / data** — list_tile(+theme), data_table(+theme/source), paginated_data_table, expansion_tile(+theme), expansion_panel, mergeable_material, reorderable_list, grid_tile(_bar), divider(+theme) | ~16 files | ~10,500 | — | ❌ | XL | ListTile, DataTable, ExpansionTile. |
| **Chips** — chip, action/choice/filter/input chip, chip_theme | ~7 files | ~5,000 | — | ❌ | L | Chip family. |
| **Progress / feedback** — progress_indicator(+theme), refresh_indicator, tooltip(+theme/visibility), badge(+theme) | ~9 files | ~3,800 | — | ❌ | L | Progress indicators, RefreshIndicator, Tooltip, Badge. |
| **Ink / Material surface** — material (981), ink_well, ink_decoration, ink_ripple/splash/sparkle/highlight, no_splash, elevation_overlay, card(+theme), circle_avatar | ~14 files | ~6,000 | — | ❌ | L | The `Material` widget + ink-splash system + Card. Foundational — most Material widgets sit on `Material`. |
| **Navigation / routing** — page_transitions_theme, predictive_back_page_transitions_builder, arc (MaterialPointArcTween), about | ~4 files | ~3,700 | — | ❌ | M | Material route transitions, AboutDialog. |
| **Search / carousel / misc** — search_anchor, search, search_bar_theme, search_view_theme, carousel(+theme), stepper, toggleable bits, adaptive_text_selection_toolbar, text_selection(+theme/toolbar+), magnifier, spell_check_suggestions_toolbar, desktop_text_selection*, autocomplete, action_icons_theme, tab_indicator, debug, constants, curves | ~25 files | ~12,500 | — | ❌ | L | SearchAnchor, Carousel, Stepper, text-selection toolbars, misc. |
| **Icons (data)** — icons.dart, animated_icons + ~12 `*.g.dart` data files, animated_icons.dart, animated_icons_data.dart | ~17 files | ~67,000 | `flui-assets` font path | ❌ data | M | Generated codepoint tables. Port = regenerate from Material font + a codegen step, not hand-translation. Low *logic* complexity, high *bulk*. |
| **Localizations** — material_localizations | 1 file | 1,473 | `flui-localizations` (new) | ❌ | M | Material l10n strings/formats. |

**Coverage estimate: ~1%.** Only a 392-LOC `ThemeData`/colors stub in `flui-app`. **`flui-material` is a brand-new crate covering ~144k LOC of logic + ~67k of icon data.** Realistically the largest single crate in the eventual workspace.

---

## 11. `cupertino` — 48,253 Dart LOC

iOS-style widget catalog. **Entirely missing — no `flui-cupertino` crate.** `icons.dart` (9,806 LOC) is generated icon data; effective logic ≈ **38,000 LOC**.

| Group | Dart files | Dart LOC (approx) | FLUI home | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- | --- |
| **App structure** — app, theme, page_scaffold, tab_scaffold, tab_view, bottom_tab_bar, nav_bar (3548 LOC), route, sheet, interface_level | ~10 files | ~10,400 | — | ❌ | XL | CupertinoApp, scaffolds, nav bar, routes, sheets. |
| **Buttons / controls** — button, switch, slider, checkbox, radio, segmented_control, sliding_segmented_control, activity_indicator | ~8 files | ~6,600 | — | ❌ | L | iOS buttons + selection controls. |
| **Input / text** — text_field (1956), text_form_field_row, search_field, text_theme, text_selection(+toolbar/button+desktop variants), magnifier, spell_check_suggestions_toolbar, adaptive_text_selection_toolbar, form_row, form_section, list_section, list_tile, expansion_tile, cupertino_focus_halo | ~22 files | ~9,500 | — | ❌ | XL | iOS text fields + selection. Blocked on RenderEditable (§8). |
| **Dialogs / menus / overlays** — dialog (2725), context_menu (1554), context_menu_action, menu_anchor (3040), picker, date_picker (2952) | ~6 files | ~13,000 | — | ❌ | XL | Cupertino dialogs, context menu, pickers, menu anchor. |
| **Feedback / misc** — refresh, scrollbar, colors (1287), localizations, thumb_painter, icon_theme_data, constants, debug, desktop_text_selection* | ~12 files | ~3,800 | — | ❌ | M | CupertinoColors (dynamic colors!), refresh, scrollbar. Dynamic-color resolution is non-trivial. |
| **Icons (data)** — icons.dart | 1 file | 9,806 | `flui-assets` font path | ❌ data | M | Generated codepoint table. |
| **Localizations** — localizations.dart | 1 file | 593 | `flui-localizations` (new) | ❌ | S | Cupertino l10n. |

**Coverage estimate: ~0%.** `flui-cupertino` is a brand-new crate, ~38k LOC of logic + ~10k icon data. Smaller than Material but still XL.

---

## 12. `services` — 30,226 Dart LOC

Platform-channel layer: text input, keyboard, clipboard, system chrome, asset bundle, restoration, mouse cursor, autofill, message codecs. **This is where the `docs/PORT.md` "binding-deletion carve-out" applies most.** Flutter's `services` is a Dart↔engine MethodChannel bridge; FLUI replaces large parts of it with native Rust crates (`flui-platform`, `flui-assets`). Note: `keyboard_key.g.dart` (5604) + `keyboard_maps.g.dart` (3204) are generated keycode tables — ~8,800 of 30,226 is data.

| Dart subsystem | Dart LOC | FLUI home (crate) | Status | Port complexity | Gap note |
| --- | --- | --- | --- | --- | --- |
| `keyboard_key.g.dart` / `keyboard_maps.g.dart` — keycode enums + platform maps | 8808 | `keyboard-types` crate dependency | 🟢 replace | S | Rust `keyboard-types` + `ui-events` crates own this. **Legitimate skip** — do not port the generated tables. |
| `hardware_keyboard.dart` / `raw_keyboard*.dart` (8 platform files) | ~4900 | `flui-platform/platforms/*/events.rs` | 🟢 replace | M | Per-platform raw-keyboard parsing replaced by `flui-platform` native event code + `ui-events`. **Mostly legitimate skip;** HardwareKeyboard state-tracking API surface may need a thin port. |
| `text_input.dart` — TextInputConnection, TextInputClient, IME | 3415 | `flui-platform` (partial), needs IME bridge | 🟡❌ | XL | IME / soft-keyboard / text-input connection. `flui-platform` has window/input but no IME composition pipeline. Major gap — prerequisite for any text field. Cannot be fully skipped; needs a Rust-native IME bridge. |
| `text_formatter.dart` / `text_editing.dart` / `text_editing_delta.dart` / `text_boundary.dart` / `text_layout_metrics.dart` / `keyboard_inserted_content.dart` | ~1800 | `flui-view` text-input layer (❌) | ❌ | L | TextInputFormatter, editing-delta model, text boundaries. Pure-logic, must be ported. |
| `platform_channel.dart` / `message_codec(s).dart` / `binary_messenger.dart` / `system_channels.dart` / `_background_isolate_*` | ~1900 | — | 🟢 mostly skip | M | The MethodChannel/codec machinery. FLUI calls Rust APIs directly — no channel layer. **Largely legitimate skip;** `system_channels` route-notification *semantics* are partly reproduced in `flui-view/binding.rs`. |
| `platform_views.dart` — embedding native views | 1719 | `flui-layer/layer/platform_view.rs` | 🟡 | L | Layer-side present; the services-side controller/registry partial. |
| `asset_bundle.dart` / `asset_manifest.dart` / `font_loader.dart` / `deferred_component.dart` | ~759 | `flui-assets` (⏸️) | ⏸️ | M | Asset loading owned by disabled `flui-assets` (4,607 Rust LOC) + `flui-engine/wgpu/font_loader.rs`. **Carve-out precedent** (`PlatformTextSystem` deletion). |
| `clipboard.dart` | 74 | `flui-platform` clipboard (`platforms/*/clipboard.rs`) | ✅ | — | Ported native. **Carve-out done.** |
| `mouse_cursor.dart` / `mouse_tracking.dart` | 1052 | `flui-platform/cursor.rs`, `flui-interaction/mouse_tracker.rs` | 🟡 | M | Cursor enum present; full cursor-manager + system-cursor mapping partial. |
| `system_chrome.dart` — status bar, orientation, system UI overlay | 799 | `flui-platform` | 🟡 | M | Partly platform-specific; some applies (orientation), some mobile-only. |
| `restoration.dart` — RestorationManager, state restoration | 1018 | — | ❌ | L | State restoration. **Possible partial-skip** (process-death recovery — mobile feature, low priority). |
| `haptic_feedback.dart` / `system_sound.dart` / `system_navigator.dart` / `live_text.dart` / `process_text.dart` / `scribe.dart` / `predictive_back_event.dart` / `sensitive_content.dart` / `flutter_version.dart` / `flavor.dart` | ~1100 | `flui-platform` | 🟡❌ | M | Small platform services; map case-by-case to `flui-platform`. Several are mobile-only. |
| `autofill.dart` | 892 | — | ❌ | M | Autofill. Platform-specific; lower priority. |
| `spell_check.dart` | 221 | — | ❌ | S | Spell-check service hook. |
| `undo_manager.dart` | 145 | `flui-view` undo (❌) | ❌ | S | System undo integration. |
| `binding.dart` / `service_extensions.dart` / `debug.dart` / `browser_context_menu.dart` | ~895 | `flui-app`, scattered | 🟡 | S | ServicesBinding shell. |

**Coverage estimate: ~35% (much of the rest is legitimate carve-out / replace).** The genuine must-port gaps inside `services`: **text-input/IME bridge** (XL) and **text-editing logic** (formatters, deltas, boundaries — L). Keyboard tables, MethodChannel codecs, raw-keyboard per-platform parsing are legitimately replaced by Rust crates and should NOT be ported.

---

## Master summary table

| Flutter package | Total Dart LOC | Effective logic LOC* | FLUI coverage estimate |
| --- | --- | --- | --- |
| foundation | 11,420 | 11,420 | ~55% |
| gestures | 14,330 | 14,330 | ~95% |
| animation | 5,283 | 5,283 | ~85% (⏸️ disabled → 0% effective) |
| physics | 893 | 893 | ~100% |
| scheduler | 2,192 | 2,192 | ~95% |
| painting | 24,890 | 24,890 | ~55% |
| semantics | 7,865 | 7,865 | ~65% |
| rendering | 52,118 | 52,118 | ~40% |
| widgets | 157,402 | 157,402 | ~25% (framework ~85%, catalog ~2%) |
| material | 210,800 | ~144,000 | ~1% |
| cupertino | 48,253 | ~38,000 | ~0% |
| services | 30,226 | ~21,400 | ~35% (much carved out) |
| **TOTAL** | **565,672** | **~479,800** | **~22%** |

\* "Effective logic LOC" subtracts generated data tables (`icons.dart`, `*.g.dart` animated-icon data, `keyboard_*.g.dart`) — those are regenerated by a codegen step, not hand-ported.

---

## Missing entirely — no FLUI home

New crates / new subsystems the roadmap must create from scratch:

1. **`flui-widgets`** — the entire user-facing widget catalog (~110k Dart LOC of `widgets` package minus the framework). Container, Row/Column/Stack/Wrap, Text/RichText, Image/Icon, ScrollView/ListView/GridView, GestureDetector, Navigator/routes, Overlay, Focus/Actions/Shortcuts widgets, AnimatedFoo/transitions, Form, MediaQuery, LayoutBuilder, FutureBuilder/StreamBuilder, etc. **The single largest greenfield block.**
2. **`flui-material`** — Material Design catalog (~144k logic LOC + ~67k icon data). ThemeData/ColorScheme, all buttons, Scaffold/AppBar/tabs/nav surfaces, TextField/InputDecorator, dialogs/sheets/menus, Checkbox/Radio/Switch/Slider, date/time pickers, ListTile/DataTable, chips, progress indicators, Material/ink system.
3. **`flui-cupertino`** — iOS catalog (~38k logic LOC + ~10k icon data). CupertinoApp, scaffolds, nav bar, buttons, text fields, dialogs, context menu, pickers, dynamic colors.
4. **`flui-localizations`** (new) — l10n infrastructure + `material_localizations` + `cupertino_localizations` (~3k LOC).
5. **`RenderViewport` + sliver render objects** (in `flui-rendering`) — RenderViewport, RenderSliverList/Grid/FixedExtentList/MultiBoxAdaptor/PersistentHeader/Padding/Fill, RenderShrinkWrappingViewport (~10k Dart LOC). Sliver *protocol* exists; render objects don't. **Blocks all scrolling.**
6. **`RenderEditable` + selection protocol** (in `flui-rendering`) — RenderEditable, SelectionRegistrar, selectable render-object protocol (~4k Dart LOC). **Blocks all text input and text selection.**
7. **`RenderParagraph`** (in `flui-rendering`) — render-object wrapper over the text-layout engine (3673 Dart LOC). `flui-painting` has the layout engine; the render object is missing. **Blocks Text widget.**
8. **~37 proxy/shifted render objects** (in `flui-rendering/objects/`) — DecoratedBox, ClipRect/RRect/Path/Oval, BackdropFilter, ColorFiltered, ShaderMask, CustomPaint, FittedBox, Align, FractionallySizedBox, MouseRegion, Listener, IgnorePointer/AbsorbPointer, RepaintBoundary, RenderStack, RenderWrap, RenderImage, RenderTable, RenderAnimatedSize, RenderRotatedBox, RenderFlow, etc. (~13k Dart LOC). Only 7 of ~80 render objects exist.
9. **Text-input / IME bridge** (in `flui-platform` + a new text-input layer) — IME composition, soft keyboard, TextInputConnection/Client equivalent, TextInputFormatter, editing deltas, text boundaries (~5k logic Dart LOC). Cannot be carved out — needs a Rust-native implementation.
10. **WidgetState controller + toggleable** (in `flui-view` or `flui-widgets`) — hover/pressed/focused/selected state tracking (`widget_state.dart` 1152 + `toggleable.dart` 672). Prerequisite for every interactive Material/Cupertino control.
11. **Diagnostics tree-dump** (in `flui-foundation`) — full `DiagnosticPropertiesBuilder`/`DiagnosticsNode` surface (`diagnostics.dart` 3707). Inspector dependency.
12. **Actions & Shortcuts system** (in `flui-widgets`) — Intent/Action/Shortcuts/key-bindings (~6k Dart LOC). Nothing ported.

---

## Disabled — needs re-enable + repair

Source exists, commented out of `[workspace.members]`. Effort is integration repair, not greenfield:

| Crate | Rust LOC | Re-enable scope |
| --- | --- | --- |
| **`flui-animation`** | 7,475 | Curves/tween/controller/simulation all present. Wire to `flui-scheduler` Ticker, verify catalog parity, fix compile against current `flui-types`/`flui-scheduler`. **Blocks all implicit/explicit animations & transitions.** Highest-priority re-enable. |
| **`flui-reactivity`** | 8,078 | Signals/computed/hooks/effects — not a Flutter port (Flutter has no signals); FLUI-original layer. Re-enable independent of Flutter parity. |
| **`flui-assets`** | 4,607 | Asset/font/image loaders + cache. Needed for Image widget + font loading. Re-enable + wire to `flui-painting`/`flui-engine`. Owns the `services` asset-bundle carve-out. |
| **`flui-devtools`** | 2,563 | Inspector backend. Depends on diagnostics tree-dump (`flui-foundation` gap). Hosts `widget_inspector.dart` equivalent. |
| **`flui-cli`** | 7,338 | Project scaffolding / build commands. Spec `001-cli-completion` exists. Independent of widget parity. |
| **`flui-build`** | 4,003 | Cross-platform build pipeline (Android/iOS/Desktop/Web). Independent of widget parity. |

---

## Legitimate skips / Rust-native replacements

Per `docs/PORT.md` "binding-deletion carve-out" — a Flutter binding is deleted, not ported, when a Rust crate stack already owns the responsibility:

- **Keyboard keycode tables** (`keyboard_key.g.dart`, `keyboard_maps.g.dart`, `raw_keyboard_*.dart` per-platform parsers — ~13k Dart LOC) → owned by `keyboard-types` + `ui-events` crates (already `flui-platform` deps). Do not port the generated tables or per-OS raw parsers.
- **MethodChannel / message-codec machinery** (`platform_channel.dart`, `message_codec(s).dart`, `binary_messenger.dart` — ~1.6k Dart LOC) → FLUI calls Rust APIs directly; there is no Dart↔engine channel. Skip the channel layer. (`system_channels` route-notification *semantics* are partly reproduced in `flui-view/binding.rs` — that part stays.)
- **`PlatformTextSystem`** — already deleted per the canonical carve-out precedent (`docs/plans/2026-03-31-platform-roadmap.md` Task 1); cosmic-text + glyphon + `flui-assets` own text shaping.
- **Clipboard** (`services/clipboard.dart`) → already native in `flui-platform/platforms/*/clipboard.rs`. Carve-out done.
- **`foundation` Dart-runtime workarounds** — `bitfield.dart`, `synchronous_future.dart`, `_isolates_*`, `persistent_hash_map.dart` (FLUI uses `TypeId` registry instead), `serialization.dart`, web-shim `_*_web.dart` files → Rust stdlib / different idiom; do not port literally.
- **`memory_allocations.dart` / `timeline.dart`** → `tracing` replaces the profiling-hook layer.
- **`physics` as a standalone package** — folded into `flui-types/src/physics/`; no `flui-physics` crate needed. (Sound consolidation — already done.)
- **Partial-skip candidates (low priority, not hard skips):** `restoration.dart` / state restoration (mobile process-death recovery), `autofill.dart`, `scribe.dart`, `sensitive_content.dart`, `live_text.dart` — platform-specific, mobile-centric; defer rather than delete.
- **Generated icon data** (`material/icons.dart`, `cupertino/icons.dart`, `animated_icons/data/*.g.dart` — ~77k LOC) — not "skipped" but **regenerated via codegen** from the Material/Cupertino icon fonts, not hand-translated. Treat as a build-step task, not a porting task.

---

## Headline numbers

Flutter's framework is **~565,700 Dart LOC across 12 packages**; subtracting ~86k of generated data tables (icon codepoints, keyboard maps, animated-icon data) leaves **~480,000 LOC of actual logic to port**. FLUI today covers roughly **22%** of that surface. The split is sharply bimodal: the **render machine and framework spine are 60–95% ported** — `gestures` (~95%), `physics` (~100%), `scheduler` (~95%), the `widgets` *framework* half in `flui-view` (~85%), and the `rendering` *engine* (RenderObject/PipelineOwner/RenderBox/layer-tree/flex). But the **user-facing layer is 0–2%**: the `widgets` *catalog* (~2%), `material` (~1%), `cupertino` (~0%). The five biggest unwritten chunks are (1) `flui-material` — a new ~144k-logic-LOC crate, the largest single body of work; (2) the `flui-widgets` catalog — ~110k Dart LOC with no crate at all; (3) `flui-cupertino` — a new ~38k-logic-LOC crate; (4) the text-input/editing/selection megasubsystem — `RenderEditable` + IME bridge + selection protocol + ~25k LOC of `editable_text`/selection widgets, all blocked behind one missing render object; (5) the scrolling subsystem — `RenderViewport` + sliver render objects + Scrollable/ListView/GridView, ~25k+ LOC blocked behind a missing render object even though the sliver *protocol* is already typed. Three surprises worth flagging to the roadmap: first, `flui-rendering/src/objects/` contains **only 7 concrete render objects** out of Flutter's ~80 — the rendering package looks far more done by LOC than it is by catalog coverage, because the *machine* is built but the *parts* aren't; second, the sliver/viewport story is a near-miss — `flui-rendering` has fully typed `SliverConstraints`/`SliverGeometry`/sliver protocol but **zero sliver render objects**, so scrolling is a "finish the job" task, not a "start from nothing" task; third, `flui-animation` is already a 7,475-LOC port (*larger* than its 5,283-LOC Dart source) sitting **disabled** — animations are closer to working than the workspace's active-crate list suggests, gated only on re-enable and integration repair.
