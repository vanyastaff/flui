# Widget → Render Object Mapping

> **Core.0 exit criterion.** Maps every planned `flui-widgets` widget to its render object(s).
> Used to scope Core.2 (render-object catalog) and Business.1 (widget catalog).
>
> Generated from Flutter source at `.flutter/flutter-master/packages/flutter/lib/src/` and
> cross-referenced with `crates/flui-rendering/src/objects/mod.rs`.

## Status Legend

- **Exists** — render object already implemented in `flui-rendering`
- **Needed** — must be built in Core.2
- **N/A** — widget does not use a dedicated render object (composes others or is pure framework)

## Summary

> **⚠ Reconciled 2026-07-01.** The original draft of this file (summary: "24 existing")
> predates the Core.0/Core.1 catalog growth. The render-object catalog now holds **71**
> concrete objects. The counts and the two lists immediately below are the **authoritative**
> status, verified against `RENDER_OBJECT_TYPES` in
> [`crates/flui-objects/tests/render_object_harness.rs`](../../crates/flui-objects/tests/render_object_harness.rs)
> and `grep`-confirmed against the source tree. **The per-widget "FLUI Status" columns in the
> body tables below are NOT re-verified row-by-row** — where they say "Needed" for an object
> that appears in the "Existing (60)" list, the list wins. The body is retained for its
> accurate Flutter-reference mapping (which Flutter RO each widget uses + the Flutter source
> file), which the Core.2 implementers need.

- **Total widgets planned:** ~87
- **Distinct concrete render objects for full parity (Core.2 target):** ~72
- **Render objects existing today:** **71**
- **Render objects remaining to build (Core.2):** **~3** (verified list below)

### Existing render objects — authoritative (71)

Concrete, harness-tested render objects (excludes base/infra types `RenderObject`, `RenderBox`, `RenderSliver`, `RenderShiftedBox`, `RenderProxyBox`, `RenderClip`, `RenderNode`):

**Box layout (26):** `RenderAlign` · `RenderAnimatedSize` · `RenderAspectRatio` · `RenderBaseline` · `RenderCenter` · `RenderConstrainedBox` · `RenderConstrainedOverflowBox` · `RenderCustomMultiChildLayoutBox` · `RenderCustomSingleChildLayoutBox` · `RenderFittedBox` · `RenderFlex` · `RenderFlow` · `RenderFractionallySizedBox` · `RenderFractionalTranslation` · `RenderIndexedStack` · `RenderIntrinsicHeight` · `RenderIntrinsicWidth` · `RenderLimitedBox` · `RenderListBody` · `RenderPadding` · `RenderRotatedBox` · `RenderSizedBox` · `RenderSizedOverflowBox` · `RenderStack` · `RenderTable` · `RenderWrap`

**Paint effects (16):** `RenderBackdropFilter` · `RenderClipOval` · `RenderClipPath` · `RenderClipRect` · `RenderClipRRect` · `RenderColoredBox` · `RenderCustomPaint` · `RenderDecoratedBox` · `RenderFollowerLayer` · `RenderLeaderLayer` · `RenderOpacity` · `RenderPhysicalModel` · `RenderPhysicalShape` · `RenderRepaintBoundary` · `RenderShaderMask` · `RenderTransform`

**Interaction / pointer (6):** `RenderAbsorbPointer` · `RenderIgnorePointer` · `RenderListener` · `RenderMetaData` · `RenderMouseRegion` · `RenderOffstage`

**Leaf (3):** `RenderEditable` · `RenderParagraph` · `RenderImage`

**Slivers + viewport (20):** `RenderViewport` · `RenderShrinkWrappingViewport` · `RenderSliverList` · `RenderSliverListLazy` · `RenderSliverGrid` · `RenderSliverGridLazy` · `RenderSliverFixedExtentList` · `RenderSliverPadding` · `RenderSliverToBoxAdapter` · `RenderSliverFillViewport` · `RenderSliverFillRemaining` · `RenderSliverFillRemainingAndOverscroll` · `RenderSliverFillRemainingWithScrollable` · `RenderSliverIgnorePointer` · `RenderSliverOffstage` · `RenderSliverOpacity` · `RenderSliverScrollingPersistentHeader` · `RenderSliverPinnedPersistentHeader` · `RenderSliverFloatingPersistentHeader` · `RenderSliverFloatingPinnedPersistentHeader`

### Remaining to build — verified missing (≈3, see `RenderAnimatedOpacity` note)

Each `grep "struct Render…"`-confirmed absent on 2026-07-01 (`RenderAnimatedSize`, the `RenderSliverPersistentHeader` family, `RenderPhysicalModel`/`RenderPhysicalShape`, `RenderBackdropFilter`/`RenderShaderMask`, and `RenderLeaderLayer`/`RenderFollowerLayer` closed same day — see closure notes below). **These three are all the Semantics family, and all genuinely need a semantics tree FLUI doesn't have yet — see the reclassification note below rather than a build plan:**

| Render object | Unblocks | Priority | Flutter source |
|---|---|---|---|
| `RenderSemanticsAnnotations` | `Semantics` | Medium (a11y) | `proxy_box.dart` |
| `RenderMergeSemantics` | `MergeSemantics` | Low | `proxy_box.dart` |
| `RenderExcludeSemantics` | `ExcludeSemantics` | Low | `proxy_box.dart` |

> **`RenderSliverGrid` closure note (verified 2026-07-01):** eager `RenderSliverGrid` and request-strategy `RenderSliverGridLazy` now ship in `flui-objects`, are listed in the render-object harness catalog, and back `SliverGrid` / `GridView.count` / `GridView.extent` / `GridView.builder`. `GridView.builder` uses the same next-frame lazy-child service model as `ListView.builder`, so first-frame blank settling remains an explicit FLUI divergence until a true mid-pass build backend exists.
>
> **`RenderShrinkWrappingViewport` closure note (verified 2026-07-01):** `RenderShrinkWrappingViewport` now ships in `flui-objects`, is listed in the harness catalog, and backs both the low-level `ShrinkWrappingViewport` widget and the high-level `CustomScrollView::shrink_wrap` / `ListView::shrink_wrap` / `GridView::shrink_wrap` composition path. It matches Flutter's bounded-cross-axis / shrink-wrapped-main-axis layout shape. Lazy `builder` views still keep FLUI's documented next-frame child-settling divergence until a true mid-pass build backend exists.
>
> **`RenderIndexedStack` closure note (verified 2026-07-01):** `RenderIndexedStack` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `IndexedStack` widget. It preserves Flutter's O(N) stack layout behavior while restricting paint, hit-test, and baseline reporting to the selected child; `index = None` displays no child.
>
> **`RenderCustomPaint` closure note (verified 2026-07-01):** `RenderCustomPaint` now ships in `flui-objects`, is listed in the harness catalog, and uses `flui-rendering`'s `CustomPainter` delegate feature. Harness coverage pins childless preferred sizing, background/child/foreground paint order, and foreground hit-test precedence. Repaint-listenable, semantics-builder, and raster-cache hint plumbing remain documented deferred edges in the render object module.
>
> **`RenderListBody` closure note (verified 2026-07-01):** `RenderListBody` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `ListBody` widget. Harness coverage pins vertical and horizontal axis-direction layout, reverse positioning, cross-axis stretching, hit testing, dry layout, and dry-baseline ordering against Flutter's `rendering/list_body.dart` behavior.
>
> **`RenderMouseRegion` closure note (verified 2026-07-01):** `RenderMouseRegion` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `MouseRegion` widget. Harness coverage pins childless `constraints.biggest` sizing, hit-entry cursor propagation, mouse-tracker annotation propagation, pointer-move hover dispatch, `MouseTracker` enter/hover/exit callbacks, and Flutter's `opaque = false` behavior: the region still contributes its hit entry while siblings visually behind it remain testable.
>
> **`RenderPointerListener` catalog note (verified 2026-07-01):** Flutter's `RenderPointerListener` is implemented as FLUI's Rust-native `RenderListener` and backs the public `Listener` widget. Harness/widget coverage pins child pass-through layout, childless live/dry `constraints.biggest` sizing, hit-entry handler propagation, `HitTestBehavior.translucent` self-entry without blocking lower siblings, down/up routing, hover routing via buttonless `PointerEvent::Move`, pointer-signal routing through FLUI's concrete `PointerEvent::Scroll`, and trackpad pan/zoom update routing through `PointerEvent::Gesture` → `PointerPanZoomEvent::Update`. Remaining edge: Flutter's pan/zoom start/end callbacks are not yet exposed on `Listener`.
>
> **`RenderEditable` first-slice note (verified 2026-07-01):** `RenderEditable` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `EditableText` widget as a single leaf render object. Harness/widget coverage pins single-line `force_line` sizing, text paint, collapsed-caret paint, self hit testing, and `EditableText` no longer splitting its text/caret into `Row` + multiple paragraphs. Deferred edges remain explicit: IME/composing, selection rendering, scrolling overflow, multiline viewport behavior, obscure text, and platform text input.
>
> **`RenderCustomSingleChildLayoutBox` closure note (verified 2026-07-01):** `RenderCustomSingleChildLayoutBox` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `CustomSingleChildLayout` widget. Harness/widget coverage pins delegated parent sizing, child constraints, child positioning, hit testing through the committed layout offset, dry layout/intrinsics, dry-baseline offsetting, and live actual-baseline forwarding. `SingleChildLayoutDelegate` is now un-gated in `flui-rendering`.
>
> **`RenderCustomMultiChildLayoutBox` closure note (verified 2026-07-01):** `RenderCustomMultiChildLayoutBox` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `CustomMultiChildLayout` widget plus `LayoutId` parent-data widget. Harness/widget coverage pins delegated parent sizing, child-id lookup, per-child constraints, layout offsets, reverse-order hit testing, dry layout/intrinsics, and `LayoutId` parent-data delivery. `MultiChildLayoutDelegate` is now un-gated in `flui-rendering`.
>
> **`RenderTable` closure note (verified 2026-07-01):** `RenderTable` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `Table`/`TableRow`/`TableCell` widgets. Building it surfaced and fixed a pre-existing type-debt bug: `TableCellParentData.vertical_alignment` was non-optional (defaulting every unset cell to `Top`), where Flutter's is nullable and defers to `RenderTable.defaultVerticalAlignment`; it is now `Option<TableCellVerticalAlignment>`, consolidated onto the single `flui_types::layout::table::TableCellVerticalAlignment` enum (retiring a duplicate `flui_rendering`-local copy). Harness coverage pins the oracle's 4-pass column-width algorithm (`Fixed`/`Flex`/`Fraction`/`Intrinsic`, including the oracle's own adversarial low-ideal/high-flex vs. high-ideal/low-flex shrink scenario), per-cell offset/size, row-decoration → children → border paint order, border interior-line placement, per-cell hit testing, and baseline row alignment. Deferred and documented: `MaxColumnWidth`/`MinColumnWidth` combinators, `TableCellVerticalAlignment::IntrinsicHeight`, RTL column ordering (matching `RenderWrap`'s/`RenderFlex`'s existing LTR-only precedent), and `TableBorder.border_radius`.
>
> **`RenderAnimatedOpacity` correction (investigated 2026-07-01):** removed from "remaining to build" — it is not a real gap. `FadeTransition` (`crates/flui-widgets/src/transitions/fade_transition.rs`) and `AnimatedOpacity` (`crates/flui-widgets/src/animated/animated_opacity.rs`) already compose the plain `Opacity` widget/`RenderOpacity`, rebuilding `Opacity::new(value)` on every animation tick — `RenderOpacity` (`crates/flui-objects/src/proxy/opacity.rs`) already implements the oracle's boundary fast paths (`paint_alpha()` → `None` and paint is skipped at alpha 0/255), so no behavior is missing. The oracle's `RenderAnimatedOpacity` differs only in *how* it stays current: it holds the `Animation<double>` itself and self-subscribes (`proxy_box.dart` `RenderAnimatedOpacityMixin`), bypassing a widget rebuild per tick — a pure performance shape, not a correctness one, and it depends on a `Listenable`/`Animation` attach-to-render-object mechanism FLUI's `RenderObject` trait does not have yet (the same gap already documented in `RenderFlow`'s and `RenderCustomPaint`'s module docs for their own painter/delegate `Listenable` wiring). Tracked as a cross-cutting FLUI-wide performance item, not a per-widget catalog gap.
>
> **`RenderAnimatedSize` investigation (2026-07-01): genuinely ADR-blocked, not just unbuilt.** Unlike `RenderAnimatedOpacity`, this is not a false positive — `AnimatedSize` does not exist in FLUI in any form (no widget, no composition, no stub; confirmed absent by grep across `flui-widgets`/`flui-objects`/`flui-animation`). Flutter's oracle `RenderAnimatedSize` (`rendering/animated_size.dart`) is architecturally different from `RenderAnimatedOpacity`'s shape: it owns and drives its **own** `AnimationController` (attached via a `vsync: TickerProvider` passed once at construction), detects a child size change during its own `performLayout`, and interpolates its reported size across many of *its own* frames — independent of any widget rebuild. This needs infrastructure that does not exist anywhere in this codebase today: (1) a lifecycle hook on FLUI's `RenderObject` trait for attach/detach-to-tree (to register/unregister a ticker), (2) a path for a render object to reach a `Vsync`/`TickerProvider` — today `Vsync` only reaches the `View`/`State` layer via `VsyncScope`, and `flui-rendering` has no dependency edge to `flui-animation` at all (crate-layering, not one-file), and (3) a way for a render-object-driven tick to trigger `markNeedsLayout` from outside the normal widget-rebuild entry point. This is a materially bigger gap than `RenderFlow`'s missing paint-time-transform primitive — it needs a cross-cutting architectural decision (an ADR) before any implementation plan can be written, since it would also be the mechanism that eventually unblocks `RenderAnimatedOpacity`'s and `RenderFlow`'s own deferred `Listenable` items. Not attempted in this pass; flagged for a chief-architect ADR rather than a build plan.
>
> **`RenderAnimatedSize` closure note (verified 2026-07-01):** the blocking architectural gap is closed by
> [`ADR-0013`](../adr/ADR-0013-render-object-attach-self-dirty-handle.md) — a defaulted `attach`/`detach`
> lifecycle pair on `RenderObject`/`RenderBox`/`RenderSliver` (mirroring the existing `reassemble` forwarded
> default), firing off the pipeline's insert/remove paths and reusing the existing `RepaintHandle`
> (extended with `mark_needs_layout`) plus `AnimationController`'s existing `Listenable` impl — no new ticker
> subsystem, no new `flui-rendering` → `flui-animation` dependency edge. `RenderAnimatedSize` now ships in
> `flui-objects` (`crates/flui-objects/src/layout/animated_size.rs`), is listed in the render-object harness
> catalog, and backs the public `AnimatedSize` widget (`crates/flui-widgets/src/animated/animated_size.rs`).
> It owns an injected (never self-built — ADR-0013 D2) `AnimationController` and subscribes to it in `attach`,
> implementing the oracle's four-state retarget machine (`Start`/`Stable`/`Changed`/`Unstable`,
> `animated_size.dart:15-51`) with its one subtlety intact: only the `Stable → Changed` transition begins the
> tween at the live current size (genuine interpolation span); every later retarget while already
> `Changed`/`Unstable` collapses to a degenerate zero-span tween (`begin = end = child's raw size`), not a
> uniform "begin = current interpolated value" — both formulas have dedicated unit + harness regression tests.
> Because it must persist its retarget state and controller subscription across rebuilds, its widget does not
> follow the sibling `AnimatedBuilder`/rebuild-per-tick convention (`AnimatedOpacity`, `AnimatedAlign`) or the
> `Align`-style whole-object-replace `update_render_object` convention — `AnimatedSizeRenderView` reaches the
> persistent render object through targeted setters only, with a widget regression test proving an unrelated
> (alignment-only) rebuild does not reset an in-flight resize animation. Deferred/documented: `reverseDuration`
> is confirmed inert (this object never runs its controller in reverse).
>
> **`RenderSliverPersistentHeader` family closure note (verified 2026-07-01):** all four
> concrete variants now ship in `crates/flui-objects/src/sliver/sliver_persistent_header.rs`
> and are listed in the render-object harness catalog: `RenderSliverScrollingPersistentHeader`
> and `RenderSliverPinnedPersistentHeader` (two small independent structs sharing a
> `PersistentHeaderCore`) and `RenderSliverFloatingPersistentHeader`/
> `RenderSliverFloatingPinnedPersistentHeader` (one generic struct over a sealed
> `FloatingHeaderMode` trait, since the oracle itself documents their `perform_layout`
> re-reveal state machine as verbatim-identical). No new ADR was needed — ADR-0013's
> `attach`/`detach` lifecycle already exists on the `RenderSliver` trait; the floating
> variants' snap-animation controller subscribes through it exactly like `RenderAnimatedSize`.
> No new `flui-rendering` delegate trait was needed either — `min_extent`/`max_extent` are
> plain constructor fields, not a delegate object (Flutter's `SliverPersistentHeaderDelegate`
> is a widget-layer, build-producing concept; Flutter's own newer `PinnedHeaderSliver`/
> `SliverFloatingHeader` widgets bypass it too). Harness coverage drives real multi-scroll-offset
> sequences through an actual `RenderViewport`: shrink/scroll-off, the floating re-reveal
> state machine's two-disjunct outer gate and two-disjunct `allow_floating_expansion`
> condition, a two-sliver test proving the pinned variant's `max_scroll_obstruction_extent`
> reaches a following sibling via `max_scroll_obstruction_extent_before`, and snap-animation
> interpolation across real controller ticks. Out of scope, documented: `show_on_screen`
> overrides (no `RenderObject::show_on_screen` exists anywhere in FLUI yet), and wiring a
> caller for `update_scroll_start_direction`/`maybe_start_snap_animation`/`maybe_stop_snap_animation`
> (needs `Scrollable`/`SliverAppBar`-layer integration, a separate future pass); the widget-layer
> `SliverPersistentHeader`/`SliverAppBar` themselves are also not in this pass.
>
> **Two pre-existing infrastructure defects discovered while building this family:**
> 1. **FIXED (2026-07-01).** `RenderViewport::attempt_layout` reported the wrong sign for
>    `constraints.overlap` (`crates/flui-objects/src/sliver/viewport.rs`, the forward-sequence
>    `overlap: center_offset.min(0.0)` line, where `center_offset = -corrected_offset`).
>    Independently re-derived against the oracle
>    (`rendering/viewport.dart:1834`: `overlap: leadingNegativeChild == null ? math.min(0.0, -centerOffset) : 0.0`,
>    with `centerOffset = mainAxisExtent * anchor - correctedOffset`; for a top-anchored
>    viewport with no leading reverse slivers this reduces to `overlap = min(0.0, correctedOffset)`)
>    and confirmed by hand: at `scroll_offset = 300` a correct top-anchored forward viewport
>    must report `overlap == 0.0`, but the old formula gave `overlap == -300.0`.
>    `RenderShrinkWrappingViewport::attempt_layout` (same file) already had the correct formula
>    (`overlap: corrected_offset.min(0.0)`) — this was a `RenderViewport`-only regression, not a
>    systemic pattern. Fixed to force `overlap == 0.0` for both sequences whenever a leading
>    reverse-growth group exists (`center_sliver_index` splits the children), matching the
>    oracle's `leadingNegativeChild != null` branch, and to `corrected_offset.min(0.0)` otherwise.
>    Two harness regression tests added (`harness_viewport_forward_overlap_is_zero_without_leading_reverse_group`,
>    `harness_viewport_reverse_group_overlap_is_always_zero`), including one through
>    `RenderSliverFillRemainingWithScrollable` (which reads `constraints.overlap` directly into
>    its `extent` formula — the sign bug inflated `extent` and silently un-clamped `paint_extent`).
>    No existing test's expected value needed to change (zero prior coverage asserted on
>    `constraints.overlap` through a real viewport).
> 2. **FIXED (2026-07-01).** No insertion path called `RenderObject::attach` for a Sliver child.
>    `PipelineOwner::insert_child_render_object` (`crates/flui-rendering/src/pipeline/owner/accessors.rs`)
>    was hard-coded to `BoxProtocol` and was the only caller of `attach_inserted_node` (the
>    ADR-0013 wiring); Sliver children were inserted via the lower-level
>    `render_tree_mut().insert_sliver_child(...)`, which never called it, and
>    `apply_deferred_mutation`'s `Insert` arm (`pipeline/owner/layout.rs:279`, the
>    lazy-list/grid-child-building path) had the same gap for **both** protocols (an additional
>    discovery beyond the original finding — the Box side of the *lazy* path was equally broken,
>    just masked because the non-lazy Box path was already correct). Fixed by adding
>    `PipelineOwner::insert_sliver_child_render_object` (the Sliver-protocol counterpart of
>    `insert_child_render_object`, same dirty-tracking + `attach_inserted_node` shape) and calling
>    it from `crate::testing::tree::mount_child`'s Sliver branch, plus adding one
>    `self.attach_inserted_node(child_id)` call in `apply_deferred_mutation`'s shared `Insert` arm
>    (protocol-generic, so it covers `DeferredRenderObject::Box` and `::Sliver` in one place).
>    `flui-view`'s real element-reconciliation path (`RenderBehavior::on_mount`) was confirmed
>    already correct and untouched — it always went through the protocol-generic `PipelineOwner::insert<P>`,
>    which already called `attach`; the gap was confined to the lazy/deferred-mutation queue and the
>    test harness. Proven red-then-green: `crates/flui-rendering/tests/attach_detach_lifecycle.rs`
>    gained a `LifecycleProbeSliver` and three tests (direct sliver-child insert,
>    deferred-sliver-insert via `apply_deferred_mutation`, and the collateral deferred-box-insert
>    case); `crates/flui-objects/tests/render_object_harness.rs`'s
>    `harness_sliver_persistent_header_floating_snap_animation_drives_effective_scroll_offset_across_ticks`
>    had its manual dirty-mark workaround removed and now proves the real `attach()`-registered
>    controller listener drives the relayout end-to-end.
>
> Both defects are independent of this render-object family's own correctness (verified by
> reading the implementation directly: the re-reveal state machine and `attach`/`detach`
> overrides are correct) but block real end-to-end floating-header snap behavior in production
> until fixed. Scoped as separate follow-up tasks, not fixed in this pass.
>
> **`RenderPhysicalModel`/`RenderPhysicalShape` closure note (verified 2026-07-01):** both now
> ship in `crates/flui-objects/src/proxy/physical_model.rs` and are listed in the render-object
> harness catalog — the render-tree primitives underneath Material elevation (Card, Dialog,
> AppBar, FAB, elevated buttons). Zero new infrastructure was needed: `Canvas::draw_shadow`
> (backed by a real analytic shadow shader, not a stub), the `RenderClip<S>` generic-collapse
> pattern, and `RRect`/`Path` fill primitives were all already shipped — this is a straight port,
> like `RenderTable`/`RenderAnimatedOpacity` before it. Uses a new, small `PhysicalClipSource`
> trait (deliberately not a reuse of `proxy::clip::ClipGeometry`, which has no room for the extra
> `shape`/`border_radius` config and no shadow/fill vocabulary) generic over one shared
> `RenderPhysicalModelBase<C>` body, monomorphized to `RenderPhysicalModel` (`BoxShape` +
> `BorderRadius`) and `RenderPhysicalShape` (arbitrary path clipper). Three confirmed divergences
> from a literal transcription, each backed by an oracle citation and a regression test: (1) the
> oracle's own `debugFillProperties` has a bug (passes `color` twice instead of `shadowColor`) —
> not reproduced, FLUI surfaces the real `shadow_color`; (2) hit-test always tests the clip shape
> for both variants (the oracle gates this on a clipper being present, which for
> `RenderPhysicalModel` specifically never happens, so a circular/rounded `RenderPhysicalModel`
> hit-tests as its full bounding box in real Flutter — FLUI applies the already-shipped
> `RenderClip<S>` "always test shape" convention instead, for FLUI-wide consistency); (3)
> `clip_behavior` defaults to `Clip::None`, not `Clip::AntiAlias` like every other class in the
> same oracle file. The highest-risk formula — the `usesSaveLayer` fork, which controls WHERE the
> fill is drawn (outside the clip on the parent canvas vs. inside via `draw_paint`), not just
> whether — is ported exactly and proven by two harness tests asserting the fill kind and count in
> each branch. `BoxShape::Circle`'s oracle formula (an ellipse — `width/2, height/2` as
> independent radii, not a true circle) is preserved even though it contradicts FLUI's own
> currently-unimplemented `BoxShape::Circle` doc comment; flagged, not silently reconciled.
> Deferred, documented: the `PhysicalModel`/`PhysicalShape` widgets and `Material`'s
> `AnimatedPhysicalModel` wrapper (a separate widget-layer pass), `debugDisableShadows`
> (confirmed debug/inspector-only), and `transparentOccluder` (confirmed inapplicable — a
> Skia-specific parameter with nothing to attach to in FLUI's from-scratch analytic shadow
> algorithm).
>
> **`RenderBackdropFilter`/`RenderShaderMask` closure note (verified 2026-07-01) — render-tree
> wiring complete, ⚠ ONE half is NOT visually working yet.** Both now ship in
> `crates/flui-objects/src/proxy/backdrop_filter.rs`/`shader_mask.rs`, as two independent
> non-generic structs (their config types, gating logic, default `blend_mode`, and diagnostics
> diverge enough that a shared generic body wasn't worth it, unlike `RenderPhysicalModel`/`Shape`).
> Building them required extending `flui-rendering`'s paint pipeline: `PaintCx` gained
> `with_shader_mask`/`with_backdrop_filter`, extending the existing closure-scoped clip mechanism
> (renamed `FragmentClip` → `FragmentScope` since it now covers non-clip effects too), and the
> composer gained two new match arms producing real `Layer::ShaderMask`/`Layer::BackdropFilter`
> nodes. `RenderBackdropFilter`'s two independent paint gates (`enabled` bypasses the filter
> entirely and still paints the child unfiltered; enabled-with-no-child paints nothing at all) and
> `RenderShaderMask`'s local-vs-global rect split (the shader callback sees the LOCAL bounds; the
> composer's existing origin-shift produces the correct global `maskRect`) are both ported exactly
> and harness-regression-tested. **Confirmed during this pass: the wgpu engine's
> `LayerRender<ShaderMaskLayer>` never actually applies the shader** — it pushes an inert
> clip-to-bounds save-layer and never reads the layer's `shader()`/`blend_mode()` fields anywhere
> in the call graph (a separate, fully-working Canvas-level shader-mask pipeline exists but is
> architecturally incompatible with `PaintCx`'s deferred-child/no-live-recursion model, so it can't
> be reused as-is). The `LayerTree` structure is correct and harness-verifiable (a real
> `Layer::ShaderMask` node with the right fields), but `ShaderMask` does not yet visually mask
> anything on screen — a confirmed `flui-engine` follow-up, not closed by this pass.
> `RenderBackdropFilter`'s GPU blur is real but covers only `ImageFilter::Blur`; other filter
> variants degrade to "children only, no backdrop effect" with a `tracing::warn!`. Scoped down from
> the oracle's current `ImageFilterConfig`/`BackdropKey` surface (bounded blur, shared-backdrop
> sampling) to the classic `filter`/`blend_mode`/`enabled` fields — neither newer feature has any
> FLUI-side backing, so building them now would be dead plumbing.
>
> **`RenderLeaderLayer`/`RenderFollowerLayer` closure note (verified 2026-07-01) — Tier 1 only;
> followers do NOT yet position correctly on screen.** Both now ship in
> `crates/flui-objects/src/proxy/leader.rs`/`follower.rs`, two independent non-generic structs
> (Leader has zero hit-test override in oracle at all; Follower has a materially different custom
> `hitTest` plus three extra fields). Extends the same `PaintCx`/`FragmentScope` closure-scoped
> mechanism `RenderShaderMask`/`RenderBackdropFilter` just proved out — `with_leader`/`with_follower`
> plus two new composer match arms producing real `Layer::Leader`/`Layer::Follower` nodes. **Key
> divergence from the immediately-preceding pair, gotten right**: both push their layer
> UNCONDITIONALLY and report `always_needs_compositing() == true` unconditionally, regardless of
> child presence — oracle never gates either on `child != null` for this family (a childless
> `CompositedTransformTarget` is still a coordinate anchor, not a visual effect), unlike
> ShaderMask/BackdropFilter's `has_child`-gated version. Regression-tested by mounting each with
> zero children and asserting the layer is still present (the direct opposite of the sibling
> pair's own no-child test).
>
> This pass precisely pinned down (not guessed) how Flutter resolves a follower's on-screen
> position independent of paint order: Flutter's `flushPaint` and `compositeFrame`/`buildScene`
> are two structurally separate phases, so a follower's transform resolves against an
> already-complete retained layer tree regardless of which subtree painted first — genuine
> same-frame, order-independent resolution, not a one-frame lag. FLUI's paint pipeline is a single
> recursive pass that directly builds the `LayerTree`, so this guarantee does not fall out "for
> free" — closing it needs a translation-only ancestor-chain-sum algorithm at RENDER time (after
> the `LayerTree` is already complete for the frame), mirroring the already-existing
> `Layer::BackdropFilter` special-case in `render_layer_recursive`. This is a real, scoped,
> tractable `flui-layer`/`flui-engine` follow-up — **not built in this pass**. Resolved-transform-
> aware hit-testing for `RenderFollowerLayer` is correctly reclassified as needing a genuine
> chief-architect ADR (mirroring ADR-0013's own precedent): `RenderObject::hit_test_transform`
> takes no external context, `PipelineOwner::hit_test` has no coupling to any `LayerTree`, and no
> `RenderId↔LayerId` correlation exists anywhere in FLUI today. `RenderFollowerLayer::hit_test`
> in this pass implements only the structural forward (has a child → hit-test it at its own
> layout-relative offset), explicitly not a self-cached shortcut. **Net effect**: the `LayerTree`
> nodes are structurally correct and harness-verifiable (fields round-trip correctly), but a
> `CompositedTransformFollower` does not yet actually render or hit-test at its resolved on-screen
> position relative to its target — this needs the deferred render-time resolution follow-up
> before it is usable in a real app.

**Core.2 entry verdict: ✓ READY.** The former critical `RenderSliverGrid` blocker is closed; the rest phase in by family off the critical path. R2 mitigated.

---

## Layout Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Container` | *(composes)* | N/A | Single | Composes Padding + DecoratedBox + ConstrainedBox + ColoredBox + etc. |
| `Padding` | `RenderPadding` | **Exists** | Single | |
| `Center` | `RenderPositionedBox` | **Exists** (as `RenderCenter`) | Single | FLUI maps to dedicated `RenderCenter`; Flutter uses `RenderPositionedBox` with default alignment |
| `Align` | `RenderPositionedBox` | **Exists** (as `RenderCenter`) | Single | Same render object as Center with configurable alignment |
| `SizedBox` | `RenderConstrainedBox` | **Exists** | Single | Uses `RenderConstrainedBox` with tight constraints |
| `ConstrainedBox` | `RenderConstrainedBox` | **Exists** | Single | |
| `LimitedBox` | `RenderLimitedBox` | **Exists** | Single | |
| `FractionallySizedBox` | `RenderFractionallySizedOverflowBox` | **Exists** (as `RenderFractionallySizedBox`) | Single | |
| `AspectRatio` | `RenderAspectRatio` | **Exists** | Single | |
| `FittedBox` | `RenderFittedBox` | **Exists** | Single | |
| `IntrinsicWidth` | `RenderIntrinsicWidth` | Needed | Single | Expensive — adds speculative layout pass |
| `IntrinsicHeight` | `RenderIntrinsicHeight` | Needed | Single | Expensive — adds speculative layout pass |
| `Baseline` | `RenderBaseline` | Needed | Single | From `shifted_box.dart` |
| `OverflowBox` | `RenderConstrainedOverflowBox` | Needed | Single | From `shifted_box.dart` |
| `Row` | `RenderFlex` | **Exists** | Variable | `FlexDirection::Horizontal` |
| `Column` | `RenderFlex` | **Exists** | Variable | `FlexDirection::Vertical` |
| `Flex` | `RenderFlex` | **Exists** | Variable | Generic flex container |
| `Expanded` | *(ParentDataWidget)* | N/A | — | Sets `FlexParentData.flex` + `FlexFit::Tight` |
| `Flexible` | *(ParentDataWidget)* | N/A | — | Sets `FlexParentData.flex` + `FlexFit::Loose` |
| `Spacer` | *(composes)* | N/A | Leaf | Wraps `Expanded(child: SizedBox.shrink())` |
| `Stack` | `RenderStack` | **Exists** | Variable | |
| `Positioned` | *(ParentDataWidget)* | N/A | — | Sets `StackParentData` offsets |
| `IndexedStack` | `RenderIndexedStack` | **Exists** | Variable | Extends `RenderStack`, shows only one child |
| `Wrap` | `RenderWrap` | Needed | Variable | From `wrap.dart` |
| `Flow` | `RenderFlow` | Needed | Variable | Paint-time transforms via `FlowDelegate` |
| `Table` | `RenderTable` | **Exists** | Variable | From `table.dart` |
| `TableRow` | *(composes)* | N/A | — | Grouping widget for Table rows |
| `CustomSingleChildLayout` | `RenderCustomSingleChildLayoutBox` | **Exists** | Single | Delegate-driven layout |
| `CustomMultiChildLayout` | `RenderCustomMultiChildLayoutBox` | **Exists** | Variable | Delegate-driven multi-child layout |
| `LayoutBuilder` | `RenderLayoutBuilder` (special) | Needed | Single | Uses `RenderObjectWithLayoutCallbackMixin` |
| `ColoredBox` | `RenderColoredBox` | **Exists** | Single | Paints colored rectangle behind child |

---

## Paint Effect Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Opacity` | `RenderOpacity` | **Exists** | Single | |
| `Transform` | `RenderTransform` | **Exists** | Single | Includes `.rotate`, `.scale`, `.translate`, `.flip` factories |
| `RotatedBox` | `RenderRotatedBox` | Needed | Single | Pre-layout rotation (vs Transform which is paint-only) |
| `FractionalTranslation` | `RenderFractionalTranslation` | **Exists** | Single | |
| `ClipRect` | `RenderClipRect` | **Exists** | Single | |
| `ClipRRect` | `RenderClipRRect` | **Exists** | Single | |
| `ClipOval` | `RenderClipOval` | **Exists** | Single | |
| `ClipPath` | `RenderClipPath` | **Exists** | Single | |
| `DecoratedBox` | `RenderDecoratedBox` | Needed | Single | Paints `BoxDecoration` (borders, gradients, shadows, images) |
| `CustomPaint` | `RenderCustomPaint` | **Exists** | Single | User-supplied foreground/background painters |
| `BackdropFilter` | `RenderBackdropFilter` | **Exists** | Single | Applies image filter to backdrop; see closure note above |
| `ShaderMask` | `RenderShaderMask` | **Exists** (render-tree only — see closure note) | Single | Applies shader as color mask; `flui-engine` does not yet visually apply it |
| `PhysicalModel` | `RenderPhysicalModel` | **Exists** | Single | Rounded-rect clip + elevation shadow; see closure note above |
| `PhysicalShape` | `RenderPhysicalShape` | **Exists** | Single | Arbitrary path clip + elevation shadow; see closure note above |
| `RepaintBoundary` | `RenderRepaintBoundary` | **Exists** | Single | Isolates repaint subtree |
| `Offstage` | `RenderOffstage` | **Exists** | Single | Hides subtree (zero-size, skip paint/hit-test) |
| `ColorFiltered` | *(composes)* | N/A | Single | Uses layer-level `ColorFilterLayer` |

---

## Sliver / Scrolling Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Scrollable` | *(framework)* | N/A | — | Orchestrates scroll physics + viewport; no own RO |
| `SingleChildScrollView` | *(composes Viewport)* | N/A | Single | Wraps child in `Viewport` + `SliverToBoxAdapter` |
| `ListView` | *(composes)* | N/A | Variable | `CustomScrollView` + `SliverList` |
| `GridView` | *(composes)* | N/A | Variable | `CustomScrollView` + `SliverGrid` |
| `CustomScrollView` | *(composes Viewport)* | N/A | Variable | Creates `Viewport` with sliver children |
| `Viewport` | `RenderViewport` | **Exists** | Variable(Sliver) | Bridge: box -> sliver protocol; from `viewport.dart` |
| `ShrinkWrappingViewport` | `RenderShrinkWrappingViewport` | **Exists** | Variable(Sliver) | Viewport that sizes to content |
| `SliverList` | `RenderSliverList` / `RenderSliverListLazy` | **Exists** | Variable(Box) | Eager and request-strategy lazy linear list paths |
| `SliverGrid` | `RenderSliverGrid` / `RenderSliverGridLazy` | **Exists** | Variable(Box) | Eager and request-strategy lazy 2D grid paths |
| `SliverFixedExtentList` | `RenderSliverFixedExtentList` | **Exists** | Variable(Box) | Eager attached-child fixed extent; lazy adaptor pending |
| `SliverFillViewport` | `RenderSliverFillViewport` | **Exists** | Variable(Box) | Each child fills viewport main-axis extent |
| `SliverToBoxAdapter` | `RenderSliverToBoxAdapter` | **Exists** | Single(Box) | Wraps single box child in sliver protocol |
| `SliverPadding` | `RenderSliverPadding` | **Exists** | Single(Sliver) | Pads a sliver child |
| `SliverAppBar` | *(composes)* | N/A | — | Material widget — uses `SliverPersistentHeader` internally |
| `SliverPersistentHeader` | `RenderSliverPersistentHeader` family | Needed | Single(Box) | Pinned/floating/scrolling persistent headers |
| `SliverOpacity` | `RenderSliverOpacity` | **Exists** | Single(Sliver) | |
| `SliverOffstage` | `RenderSliverOffstage` | **Exists** | Single(Sliver) | |
| `SliverIgnorePointer` | `RenderSliverIgnorePointer` | **Exists** | Single(Sliver) | |

---

## Input / Gesture Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `GestureDetector` | *(composes)* | N/A | Single | Composes `Listener` + gesture recognizers; no own RO |
| `Listener` | `RenderPointerListener` | **Exists** (as `RenderListener`) | Single | Raw pointer callbacks; buttonless move maps to hover, scroll maps to pointer signals, gesture maps to pan/zoom updates |
| `MouseRegion` | `RenderMouseRegion` | **Exists** | Single | Mouse hover enter/exit tracking |
| `AbsorbPointer` | `RenderAbsorbPointer` | **Exists** | Single | Catches hits, blocks child |
| `IgnorePointer` | `RenderIgnorePointer` | **Exists** | Single | Pointers pass through subtree |
| `Focus` | *(framework)* | N/A | Single | Focus node management; no own RO |
| `FocusScope` | *(framework)* | N/A | Single | Focus scope management; no own RO |
| `Actions` | *(framework)* | N/A | Single | Intent→Action dispatch; no own RO |
| `Shortcuts` | *(framework)* | N/A | Single | Key combo → Intent mapping; no own RO |

---

## Text Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `RichText` | `RenderParagraph` | **Exists** | Leaf | Core text rendering; drives cosmic-text in FLUI |
| `Text` | *(composes)* | N/A | Leaf | Wraps `RichText` with `DefaultTextStyle` |
| `DefaultTextStyle` | *(InheritedWidget)* | N/A | Single | Provides inherited text style; no own RO |
| `EditableText` | `RenderEditable` | **Exists first visual slice** | Leaf | Text + collapsed cursor; selection, IME, scrolling overflow, multiline deferred |

---

## Image Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Image` | `RenderImage` | **Exists** | Leaf | Displays decoded image with fit/alignment |
| `Icon` | *(composes)* | N/A | Leaf | Composes `RichText` with icon font glyph |

---

## Animation Widgets

### Explicit Animations (Transitions)

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `FadeTransition` | *(composes)* | N/A | Single | Composes `Opacity`/`RenderOpacity`, rebuilt per tick — see the `RenderAnimatedOpacity` correction note above |
| `SlideTransition` | *(composes)* | N/A | Single | Composes `FractionalTranslation` driven by animation |
| `ScaleTransition` | *(composes)* | N/A | Single | Composes `Transform.scale` driven by animation |
| `RotationTransition` | *(composes)* | N/A | Single | Composes `Transform.rotate` driven by animation |
| `SizeTransition` | *(composes)* | N/A | Single | Composes `ClipRect` + `Align` driven by animation |
| `AnimatedBuilder` | *(framework)* | N/A | Single | Rebuilds child on `Listenable` notification; no own RO |

### Implicit Animations

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `AnimatedContainer` | *(composes)* | N/A | Single | Implicitly animates Container properties |
| `AnimatedPadding` | *(composes)* | N/A | Single | Implicitly animates Padding |
| `AnimatedOpacity` | *(composes)* | N/A | Single | Composes `Opacity`/`RenderOpacity`, rebuilt per tick — see the `RenderAnimatedOpacity` correction note above |
| `AnimatedPositioned` | *(composes)* | N/A | Single | Implicitly animates Positioned in Stack |
| `AnimatedAlign` | *(composes)* | N/A | Single | Implicitly animates Align |
| `AnimatedDefaultTextStyle` | *(composes)* | N/A | Single | Implicitly animates DefaultTextStyle |
| `AnimatedSize` | `RenderAnimatedSize` | **Exists** | Single | Animates size changes over time; see ADR-0013 closure note above |

### Hero

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Hero` | *(framework)* | N/A | Single | Cross-route flight animation; reparents via `Overlay`; no own RO |

---

## Navigation / Routing Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Navigator` | *(framework)* | N/A | Variable | Route stack management; no own RO |
| `PageRoute` | *(framework)* | N/A | — | Abstract route with transition; no own RO |
| `Overlay` | *(framework)* | N/A | Variable | Manages `OverlayEntry` stack above routes |
| `OverlayEntry` | *(framework)* | N/A | Single | Single entry in the overlay |

---

## Miscellaneous Framework Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Visibility` | *(composes)* | N/A | Single | Composes `Offstage` + `IgnorePointer` + `TickerMode` |
| `Semantics` | `RenderSemanticsAnnotations` | Needed | Single | Accessibility annotation proxy |
| `MergeSemantics` | `RenderMergeSemantics` | Needed | Single | Merges child semantics into one node |
| `ExcludeSemantics` | `RenderExcludeSemantics` | Needed | Single | Drops child semantics |
| `Builder` | *(framework)* | N/A | Single | Convenience StatelessWidget with inline builder; no own RO |
| `MediaQuery` | *(InheritedWidget)* | N/A | Single | Provides device metrics; no own RO |
| `InheritedWidget` | *(framework)* | N/A | Single | Data-sharing base class; no own RO |
| `InheritedModel` | *(framework)* | N/A | Single | Aspect-selective inherited data; no own RO |
| `ValueListenableBuilder` | *(framework)* | N/A | Single | Rebuilds on `ValueListenable` change; no own RO |
| `FutureBuilder` | *(framework)* | N/A | Single | Rebuilds on `Future` completion; no own RO |
| `StreamBuilder` | *(framework)* | N/A | Single | Rebuilds on `Stream` events; no own RO |
| `TickerProvider` | *(framework)* | N/A | — | Mixin providing `Ticker` for animations; no own RO |
| `MetaData` | `RenderMetaData` | **Exists** | Single | Attaches opaque data to hit-test entries |
| `CompositedTransformTarget` | `RenderLeaderLayer` | **Exists** | Single | Anchor for follower layer; see closure note above |
| `CompositedTransformFollower` | `RenderFollowerLayer` | **Exists** (render-tree only — see closure note) | Single | Follows leader layer position; on-screen resolution not yet wired |
| `ListBody` | `RenderListBody` | **Exists** | Variable | Sequential body layout (used by `Dialog`) |

---

## Core.2 Build Checklist — Render Objects To Implement

Grouped by family for parallelizable construction (per ROADMAP Core.2 structure):

### Wave 1 — Box Layout (4 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderIntrinsicWidth` | `proxy_box.dart` | `IntrinsicWidth` |
| 2 | `RenderIntrinsicHeight` | `proxy_box.dart` | `IntrinsicHeight` |
| 3 | `RenderBaseline` | `shifted_box.dart` | `Baseline` |
| 4 | `RenderConstrainedOverflowBox` | `shifted_box.dart` | `OverflowBox` |

### Wave 2 — Multi-Child Layout (3 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderWrap` | `wrap.dart` | `Wrap` |
| 2 | `RenderFlow` | `flow.dart` | `Flow` |
| 3 | `RenderTable` | `table.dart` | `Table` |

### Wave 3 — Paint Effects (6 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderDecoratedBox` | `proxy_box.dart` | `DecoratedBox`, `Container` |
| 2 | `RenderBackdropFilter` | `proxy_box.dart` | `BackdropFilter` |
| 3 | `RenderShaderMask` | `proxy_box.dart` | `ShaderMask` |
| 4 | `RenderPhysicalModel` | `proxy_box.dart` | `PhysicalModel` |
| 5 | `RenderPhysicalShape` | `proxy_box.dart` | `PhysicalShape` |
| 6 | `RenderRotatedBox` | `rotated_box.dart` | `RotatedBox` |

### Wave 4 — Input / Leaf (0 objects remaining)

`RenderEditable` exists as the single-line visual core for `EditableText`. Full
IME/selection/scrolling behavior is App.1/platform work, not a missing render
object.

### Wave 5 — Slivers / Viewport (8 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderViewport` | `viewport.dart` | `Viewport`, `CustomScrollView`, `ListView`, `GridView` |
| 2 | `RenderShrinkWrappingViewport` | `viewport.dart` | `ShrinkWrappingViewport` |
| 3 | `RenderSliverList` | `sliver_list.dart` | `SliverList`, `ListView` |
| 4 | `RenderSliverGrid` | `sliver_grid.dart` | `SliverGrid`, `GridView` |
| 5 | `RenderSliverFixedExtentList` | `sliver_fixed_extent_list.dart` | `SliverFixedExtentList` |
| 6 | `RenderSliverFillViewport` | `sliver_fill.dart` | `SliverFillViewport` |
| 7 | `RenderSliverToBoxAdapter` | `sliver.dart` | `SliverToBoxAdapter`, `SingleChildScrollView` |
| 8 | `RenderSliverPersistentHeader` | `sliver_persistent_header.dart` | `SliverPersistentHeader` |

### Wave 6 — Animation + Semantics + Misc (6 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderAnimatedOpacity` | `proxy_box.dart` | `FadeTransition`, `AnimatedOpacity` |
| 2 | `RenderAnimatedSize` | `animated_size.dart` | `AnimatedSize` |
| 3 | `RenderSemanticsAnnotations` | `proxy_box.dart` | `Semantics` |
| 4 | `RenderMergeSemantics` | `proxy_box.dart` | `MergeSemantics` |
| 5 | `RenderLeaderLayer` | `proxy_box.dart` | `CompositedTransformTarget` |
| 6 | `RenderFollowerLayer` | `proxy_box.dart` | `CompositedTransformFollower` |

### Wave 7 — Secondary (1 object)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderExcludeSemantics` | `proxy_box.dart` | `ExcludeSemantics` |

---

## Appendix A — Arity Classification Key

| Arity | Meaning | Rust Type |
|-------|---------|-----------|
| Leaf | No children | — |
| Single | Exactly one child | `Option<RenderId>` |
| Optional | Zero or one child | `Option<RenderId>` |
| Variable | Zero or more children | `Vec<RenderId>` / slab linked-list |

## Appendix B — Widgets Not Requiring Dedicated Render Objects

These widgets are either pure-framework (StatelessWidget / StatefulWidget / InheritedWidget subclasses that compose other widgets) or ParentDataWidgets that configure parent-data on an existing render object:

| Widget | Reason |
|--------|--------|
| `Container` | Composes Padding + DecoratedBox + ConstrainedBox + ColoredBox + Align + Transform |
| `Expanded` / `Flexible` | ParentDataWidget — sets `FlexParentData` on parent `RenderFlex` |
| `Positioned` | ParentDataWidget — sets `StackParentData` on parent `RenderStack` |
| `Spacer` | Composes `Expanded(child: SizedBox.shrink())` |
| `Center` | Alias for `Align(alignment: Alignment.center)` |
| `Text` | Wraps `RichText` + `DefaultTextStyle` |
| `Icon` | Composes `RichText` with icon font glyph |
| `GestureDetector` | Composes `Listener` + gesture recognizer |
| `Visibility` | Composes `Offstage` + `IgnorePointer` + `TickerMode` |
| `ListView` / `GridView` | Compose `CustomScrollView` + slivers |
| `SingleChildScrollView` | Composes `Scrollable` + `Viewport` + `SliverToBoxAdapter` |
| `Navigator` / `PageRoute` / `Overlay` | Pure framework (route stack, overlay management) |
| `Hero` | Framework orchestration (flight controller + Overlay) |
| `Builder` / `LayoutBuilder` | Builder uses inline callback; LayoutBuilder uses special RO with callback |
| `MediaQuery` / `InheritedWidget` / `InheritedModel` | InheritedWidget subclasses — data sharing |
| `FutureBuilder` / `StreamBuilder` / `ValueListenableBuilder` | Stateful widgets that rebuild on async events |
| `AnimatedContainer` / `AnimatedPadding` / etc. | ImplicitlyAnimatedWidget — interpolates then composes real widgets |
| `SlideTransition` / `ScaleTransition` / `RotationTransition` | AnimatedWidget — drives Transform/FractionalTranslation children |
| `Focus` / `FocusScope` / `Actions` / `Shortcuts` | Framework-level focus and action management |
| `DefaultTextStyle` | InheritedWidget providing text style |
| `Scrollable` | Framework controller for scroll interaction |
| `ColorFiltered` | Layer-level effect (pushes `ColorFilterLayer`) |

## Appendix C — Flutter Render Object → Source File Index

For Core.2 implementers — the Flutter source file to reference for each render object:

| Render Object | Flutter Source |
|---|---|
| `RenderPadding` | `shifted_box.dart` |
| `RenderPositionedBox` | `shifted_box.dart` |
| `RenderBaseline` | `shifted_box.dart` |
| `RenderConstrainedOverflowBox` | `shifted_box.dart` |
| `RenderFractionallySizedOverflowBox` | `shifted_box.dart` |
| `RenderSizedOverflowBox` | `shifted_box.dart` |
| `RenderConstrainedBox` | `proxy_box.dart` |
| `RenderLimitedBox` | `proxy_box.dart` |
| `RenderAspectRatio` | `proxy_box.dart` |
| `RenderIntrinsicWidth` / `Height` | `proxy_box.dart` |
| `RenderOpacity` | `proxy_box.dart` |
| `RenderAnimatedOpacity` | `proxy_box.dart` |
| `RenderTransform` | `proxy_box.dart` |
| `RenderFittedBox` | `proxy_box.dart` |
| `RenderFractionalTranslation` | `proxy_box.dart` |
| `RenderClipRect` / `RRect` / `Oval` / `Path` | `proxy_box.dart` |
| `RenderPhysicalModel` / `Shape` | `proxy_box.dart` |
| `RenderDecoratedBox` | `proxy_box.dart` |
| `RenderBackdropFilter` | `proxy_box.dart` |
| `RenderShaderMask` | `proxy_box.dart` |
| `RenderRepaintBoundary` | `proxy_box.dart` |
| `RenderOffstage` | `proxy_box.dart` |
| `RenderAbsorbPointer` | `proxy_box.dart` |
| `RenderIgnorePointer` | `proxy_box.dart` |
| `RenderMetaData` | `proxy_box.dart` |
| `RenderPointerListener` | `proxy_box.dart` |
| `RenderMouseRegion` | `proxy_box.dart` |
| `RenderSemanticsAnnotations` | `proxy_box.dart` |
| `RenderMergeSemantics` | `proxy_box.dart` |
| `RenderExcludeSemantics` | `proxy_box.dart` |
| `RenderLeaderLayer` | `proxy_box.dart` |
| `RenderFollowerLayer` | `proxy_box.dart` |
| `RenderCustomPaint` | `custom_paint.dart` |
| `RenderCustomSingleChildLayoutBox` | `custom_layout.dart` |
| `RenderCustomMultiChildLayoutBox` | `custom_layout.dart` |
| `RenderFlex` | `flex.dart` |
| `RenderStack` / `RenderIndexedStack` | `stack.dart` |
| `RenderWrap` | `wrap.dart` |
| `RenderFlow` | `flow.dart` |
| `RenderTable` | `table.dart` |
| `RenderListBody` | `list_body.dart` |
| `RenderRotatedBox` | `rotated_box.dart` |
| `RenderAnimatedSize` | `animated_size.dart` |
| `RenderParagraph` | `paragraph.dart` |
| `RenderEditable` | `editable.dart` |
| `RenderImage` | `image.dart` |
| `RenderViewport` / `ShrinkWrapping` | `viewport.dart` |
| `RenderSliverList` | `sliver_list.dart` |
| `RenderSliverGrid` | `sliver_grid.dart` |
| `RenderSliverFixedExtentList` | `sliver_fixed_extent_list.dart` |
| `RenderSliverFillViewport` | `sliver_fill.dart` |
| `RenderSliverToBoxAdapter` | `sliver.dart` |
| `RenderSliverPersistentHeader` | `sliver_persistent_header.dart` |
| `RenderSliverPadding` | `sliver_padding.dart` |
| `RenderSliverOpacity` | `proxy_sliver.dart` |
| `RenderSliverIgnorePointer` | `proxy_sliver.dart` |
| `RenderSliverOffstage` | `proxy_sliver.dart` |
| `RenderColoredBox` | *(FLUI-original leaf)* |
| `RenderSizedBox` | *(FLUI-original leaf)* |
