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

- **Total widgets planned:** 87
- **Distinct render objects needed:** 48
- **Render objects existing:** 24
- **Render objects to build in Core.2:** 24

### Existing render objects (24)

| # | Render Object | FLUI Module | Wave |
|---|---|---|---|
| 1 | `RenderColoredBox` | `objects::colored_box` | — |
| 2 | `RenderSizedBox` | `objects::sized_box` | — |
| 3 | `RenderPadding` | `objects::padding` | — |
| 4 | `RenderCenter` | `objects::center` | — |
| 5 | `RenderOpacity` | `objects::opacity` | — |
| 6 | `RenderTransform` | `objects::transform` | — |
| 7 | `RenderConstrainedBox` | `objects::constrained_box` | Core.2 |
| 8 | `RenderLimitedBox` | `objects::limited_box` | Core.2 |
| 9 | `RenderAspectRatio` | `objects::aspect_ratio` | Core.2 |
| 10 | `RenderFractionallySizedBox` | `objects::fractionally_sized_box` | Core.2 |
| 11 | `RenderClipRect` | `objects::clip` | Core.2 |
| 12 | `RenderClipRRect` | `objects::clip` | Core.2 |
| 13 | `RenderClipOval` | `objects::clip` | Core.2 |
| 14 | `RenderClipPath` | `objects::clip` | Core.2 |
| 15 | `RenderRepaintBoundary` | `objects::repaint_boundary` | Core.2 |
| 16 | `RenderOffstage` | `objects::offstage` | Core.2 W4 |
| 17 | `RenderAbsorbPointer` | `objects::absorb_pointer` | Core.2 W4 |
| 18 | `RenderIgnorePointer` | `objects::ignore_pointer` | Core.2 W4 |
| 19 | `RenderMetaData` | `objects::meta_data` | Core.2 W4 |
| 20 | `RenderFractionalTranslation` | `objects::fractional_translation` | Core.2 W4 |
| 21 | `RenderFittedBox` | `objects::fitted_box` | Core.2 W4 |
| 22 | `RenderFlex` | `objects::flex` | — |
| 23 | `RenderStack` | `objects::stack` | Core.2 W2a |
| 24 | `RenderSliverPadding` | `objects::sliver_padding` | Core.2 W5a |
| 25 | `RenderSliverOpacity` | `objects::sliver_opacity` | Core.2 W5a |
| 26 | `RenderSliverIgnorePointer` | `objects::sliver_ignore_pointer` | Core.2 W5a |
| 27 | `RenderSliverOffstage` | `objects::sliver_offstage` | Core.2 W5a |

> Note: 27 entries above — the summary counts "24 existing" as those used directly by widgets; the sliver proxies (24–27) are infrastructure but listed for completeness.

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
| `IndexedStack` | `RenderIndexedStack` | Needed | Variable | Extends `RenderStack`, shows only one child |
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
| `CustomPaint` | `RenderCustomPaint` | Needed | Single | User-supplied foreground/background painters |
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
| `Viewport` | `RenderViewport` | Needed | Variable(Sliver) | Bridge: box → sliver protocol; from `viewport.dart` |
| `ShrinkWrappingViewport` | `RenderShrinkWrappingViewport` | Needed | Variable(Sliver) | Viewport that sizes to content |
| `SliverList` | `RenderSliverList` | Needed | Variable(Box) | Lazy linear list of box children |
| `SliverGrid` | `RenderSliverGrid` | Needed | Variable(Box) | Lazy 2D grid of box children |
| `SliverFixedExtentList` | `RenderSliverFixedExtentList` | **Exists** | Variable(Box) | Eager attached-child fixed extent; lazy adaptor pending |
| `SliverFillViewport` | `RenderSliverFillViewport` | Needed | Variable(Box) | Each child fills viewport main-axis extent |
| `SliverToBoxAdapter` | `RenderSliverToBoxAdapter` | Needed | Single(Box) | Wraps single box child in sliver protocol |
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
| `Listener` | `RenderPointerListener` | Needed | Single | Raw pointer event callbacks |
| `MouseRegion` | `RenderMouseRegion` | Needed | Single | Mouse hover enter/exit tracking |
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
| `RichText` | `RenderParagraph` | Needed | Leaf | Core text rendering; drives cosmic-text in FLUI |
| `Text` | *(composes)* | N/A | Leaf | Wraps `RichText` with `DefaultTextStyle` |
| `DefaultTextStyle` | *(InheritedWidget)* | N/A | Single | Provides inherited text style; no own RO |
| `EditableText` | `RenderEditable` | Needed | Leaf | Text editing with cursor, selection, IME |

---

## Image Widgets

| Widget | Flutter RenderObject | FLUI Status | Arity | Notes |
|--------|---------------------|-------------|-------|-------|
| `Image` | `RenderImage` | Needed | Leaf | Displays decoded image with fit/alignment |
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
| `ListBody` | `RenderListBody` | Needed | Variable | Sequential body layout (used by `Dialog`) |

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

### Wave 2 — Multi-Child Layout (4 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderIndexedStack` | `stack.dart` | `IndexedStack` |
| 2 | `RenderWrap` | `wrap.dart` | `Wrap` |
| 3 | `RenderFlow` | `flow.dart` | `Flow` |
| 4 | `RenderTable` | `table.dart` | `Table` |

### Wave 3 — Paint Effects (7 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderDecoratedBox` | `proxy_box.dart` | `DecoratedBox`, `Container` |
| 2 | `RenderCustomPaint` | `custom_paint.dart` | `CustomPaint` |
| 3 | `RenderBackdropFilter` | `proxy_box.dart` | `BackdropFilter` |
| 4 | `RenderShaderMask` | `proxy_box.dart` | `ShaderMask` |
| 5 | `RenderPhysicalModel` | `proxy_box.dart` | `PhysicalModel` |
| 6 | `RenderPhysicalShape` | `proxy_box.dart` | `PhysicalShape` |
| 7 | `RenderRotatedBox` | `rotated_box.dart` | `RotatedBox` |

### Wave 4 — Input / Leaf (5 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderPointerListener` | `proxy_box.dart` | `Listener` |
| 2 | `RenderMouseRegion` | `proxy_box.dart` | `MouseRegion` |
| 3 | `RenderParagraph` | `paragraph.dart` | `RichText`, `Text` |
| 4 | `RenderEditable` | `editable.dart` | `EditableText` |
| 5 | `RenderImage` | `image.dart` | `Image` |

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

### Wave 7 — Secondary (2 objects)

| # | Render Object | Flutter File | Needed By Widgets |
|---|---|---|---|
| 1 | `RenderListBody` | `list_body.dart` | `ListBody` |
| 2 | `RenderExcludeSemantics` | `proxy_box.dart` | `ExcludeSemantics` |

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
