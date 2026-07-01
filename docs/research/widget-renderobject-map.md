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
> predates the Core.0/Core.1 catalog growth. The render-object catalog now holds **55**
> concrete objects. The counts and the two lists immediately below are the **authoritative**
> status, verified against `RENDER_OBJECT_TYPES` in
> [`crates/flui-objects/tests/render_object_harness.rs`](../../crates/flui-objects/tests/render_object_harness.rs)
> and `grep`-confirmed against the source tree. **The per-widget "FLUI Status" columns in the
> body tables below are NOT re-verified row-by-row** — where they say "Needed" for an object
> that appears in the "Existing (55)" list, the list wins. The body is retained for its
> accurate Flutter-reference mapping (which Flutter RO each widget uses + the Flutter source
> file), which the Core.2 implementers need.

- **Total widgets planned:** ~87
- **Distinct concrete render objects for full parity (Core.2 target):** ~72
- **Render objects existing today:** **55**
- **Render objects remaining to build (Core.2):** **~17** (verified list below)

### Existing render objects — authoritative (55)

Concrete, harness-tested render objects (excludes base/infra types `RenderObject`, `RenderBox`, `RenderSliver`, `RenderShiftedBox`, `RenderProxyBox`, `RenderClip`, `RenderNode`):

**Box layout (21):** `RenderAlign` · `RenderAspectRatio` · `RenderBaseline` · `RenderCenter` · `RenderConstrainedBox` · `RenderConstrainedOverflowBox` · `RenderFittedBox` · `RenderFlex` · `RenderFractionallySizedBox` · `RenderFractionalTranslation` · `RenderIndexedStack` · `RenderIntrinsicHeight` · `RenderIntrinsicWidth` · `RenderLimitedBox` · `RenderListBody` · `RenderPadding` · `RenderRotatedBox` · `RenderSizedBox` · `RenderSizedOverflowBox` · `RenderStack` · `RenderWrap`

**Paint effects (10):** `RenderClipOval` · `RenderClipPath` · `RenderClipRect` · `RenderClipRRect` · `RenderColoredBox` · `RenderCustomPaint` · `RenderDecoratedBox` · `RenderOpacity` · `RenderRepaintBoundary` · `RenderTransform`

**Interaction / pointer (6):** `RenderAbsorbPointer` · `RenderIgnorePointer` · `RenderListener` · `RenderMetaData` · `RenderMouseRegion` · `RenderOffstage`

**Leaf (2):** `RenderParagraph` · `RenderImage`

**Slivers + viewport (16):** `RenderViewport` · `RenderShrinkWrappingViewport` · `RenderSliverList` · `RenderSliverListLazy` · `RenderSliverGrid` · `RenderSliverGridLazy` · `RenderSliverFixedExtentList` · `RenderSliverPadding` · `RenderSliverToBoxAdapter` · `RenderSliverFillViewport` · `RenderSliverFillRemaining` · `RenderSliverFillRemainingAndOverscroll` · `RenderSliverFillRemainingWithScrollable` · `RenderSliverIgnorePointer` · `RenderSliverOffstage` · `RenderSliverOpacity`

### Remaining to build — verified missing (≈17)

Each `grep "struct Render…"`-confirmed absent on 2026-07-01:

| Render object | Unblocks | Priority | Flutter source |
|---|---|---|---|
| `RenderEditable` | `EditableText`/`TextField` | High (App.1 IME) | `editable.dart` |
| `RenderFlow` | `Flow` | Medium | `flow.dart` |
| `RenderTable` | `Table`/`DataTable` | Medium | `table.dart` |
| `RenderCustomSingleChildLayoutBox` | `CustomSingleChildLayout` | Medium | `custom_layout.dart` |
| `RenderCustomMultiChildLayoutBox` | `CustomMultiChildLayout` | Medium | `custom_layout.dart` |
| `RenderSliverPersistentHeader` family | `SliverAppBar`/pinned headers | Medium | `sliver_persistent_header.dart` |
| `RenderAnimatedOpacity` | `FadeTransition`/`AnimatedOpacity` | Medium | `proxy_box.dart` |
| `RenderAnimatedSize` | `AnimatedSize` | Medium | `animated_size.dart` |
| `RenderBackdropFilter` | `BackdropFilter` | Low | `proxy_box.dart` |
| `RenderShaderMask` | `ShaderMask` | Low | `proxy_box.dart` |
| `RenderPhysicalModel` | `PhysicalModel` | Low (Material elevation) | `proxy_box.dart` |
| `RenderPhysicalShape` | `PhysicalShape` | Low | `proxy_box.dart` |
| `RenderSemanticsAnnotations` | `Semantics` | Medium (a11y) | `proxy_box.dart` |
| `RenderMergeSemantics` | `MergeSemantics` | Low | `proxy_box.dart` |
| `RenderExcludeSemantics` | `ExcludeSemantics` | Low | `proxy_box.dart` |
| `RenderLeaderLayer` | `CompositedTransformTarget` | Low | `proxy_box.dart` |
| `RenderFollowerLayer` | `CompositedTransformFollower` | Low | `proxy_box.dart` |

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
> **`RenderMouseRegion` closure note (verified 2026-07-01):** `RenderMouseRegion` now ships in `flui-objects`, is listed in the render-object harness catalog, and backs the public `MouseRegion` widget. Harness coverage pins childless `constraints.biggest` sizing, hit-entry cursor propagation, mouse-tracker annotation propagation, pointer-move hover dispatch, and `MouseTracker` enter/hover/exit callbacks. Remaining edge: Flutter's `opaque = false` lets regions behind it remain active while this region still contributes an annotation; FLUI's current hit-test pipeline still couples "add self hit entry" with "blocks siblings below", so transparent behind-region behavior remains an explicit pipeline follow-up.
>
> **`RenderPointerListener` catalog note (verified 2026-07-01):** Flutter's `RenderPointerListener` is implemented as FLUI's Rust-native `RenderListener` and backs the public `Listener` widget. Harness/widget coverage pins child pass-through layout, childless live/dry `constraints.biggest` sizing, hit-entry handler propagation, down/up routing, hover routing via buttonless `PointerEvent::Move`, pointer-signal routing through FLUI's concrete `PointerEvent::Scroll`, and trackpad pan/zoom update routing through `PointerEvent::Gesture` → `PointerPanZoomEvent::Update`. Remaining edges: Flutter's pan/zoom start/end callbacks are not yet exposed on `Listener`, and `HitTestBehavior.translucent` registers self without blocking siblings behind; FLUI's current hit-test pipeline still couples "add self hit entry" with "blocks siblings below", so translucent behind-target behavior remains an explicit pipeline follow-up.

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
| `Table` | `RenderTable` | Needed | Variable | From `table.dart` |
| `TableRow` | *(composes)* | N/A | — | Grouping widget for Table rows |
| `CustomSingleChildLayout` | `RenderCustomSingleChildLayoutBox` | Needed | Single | Delegate-driven layout |
| `CustomMultiChildLayout` | `RenderCustomMultiChildLayoutBox` | Needed | Variable | Delegate-driven multi-child layout |
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
| `BackdropFilter` | `RenderBackdropFilter` | Needed | Single | Applies image filter to backdrop |
| `ShaderMask` | `RenderShaderMask` | Needed | Single | Applies shader as color mask |
| `PhysicalModel` | `RenderPhysicalModel` | Needed | Single | Rounded-rect clip + elevation shadow |
| `PhysicalShape` | `RenderPhysicalShape` | Needed | Single | Arbitrary path clip + elevation shadow |
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
| `EditableText` | `RenderEditable` | Needed | Leaf | Text editing with cursor, selection, IME |

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
| `FadeTransition` | `RenderAnimatedOpacity` | Needed | Single | Animated opacity via `Animation<double>` |
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
| `AnimatedOpacity` | `RenderAnimatedOpacity` | Needed | Single | Implicit opacity animation |
| `AnimatedPositioned` | *(composes)* | N/A | Single | Implicitly animates Positioned in Stack |
| `AnimatedAlign` | *(composes)* | N/A | Single | Implicitly animates Align |
| `AnimatedDefaultTextStyle` | *(composes)* | N/A | Single | Implicitly animates DefaultTextStyle |
| `AnimatedSize` | `RenderAnimatedSize` | Needed | Single | Animates size changes over time |

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
| `CompositedTransformTarget` | `RenderLeaderLayer` | Needed | Single | Anchor for follower layer |
| `CompositedTransformFollower` | `RenderFollowerLayer` | Needed | Single | Follows leader layer position |
| `ListBody` | `RenderListBody` | **Exists** | Variable | Sequential body layout (used by `Dialog`) |

---

## Core.2 Build Checklist — Render Objects To Implement

Grouped by family for parallelizable construction (per ROADMAP Core.2 structure):

### Wave 1 — Box Layout (6 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderIntrinsicWidth` | `proxy_box.dart` | `IntrinsicWidth` |
| 2 | `RenderIntrinsicHeight` | `proxy_box.dart` | `IntrinsicHeight` |
| 3 | `RenderBaseline` | `shifted_box.dart` | `Baseline` |
| 4 | `RenderConstrainedOverflowBox` | `shifted_box.dart` | `OverflowBox` |
| 5 | `RenderCustomSingleChildLayoutBox` | `custom_layout.dart` | `CustomSingleChildLayout` |
| 6 | `RenderCustomMultiChildLayoutBox` | `custom_layout.dart` | `CustomMultiChildLayout` |

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

### Wave 4 — Input / Leaf (1 object remaining)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderEditable` | `editable.dart` | `EditableText` |

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
