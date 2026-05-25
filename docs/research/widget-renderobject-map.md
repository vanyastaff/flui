# Widget → Render-Object Mapping Checklist

> **Status:** seeded by Core.2 Wave 1 (constraint family). Subsequent
> waves extend this table. Per `docs/ROADMAP.md` Core.0 exit criteria,
> this document is the canonical mapping that gates Core.2 entry — once
> every planned `flui-widgets` widget appears here with a green
> render-object backing, Business.1 has no hidden bottleneck.

This table maps every planned `flui-widgets` widget (and the design-system
widgets in `flui-material` / `flui-cupertino` that compose them) to its
backing render object in `crates/flui-rendering/src/objects/`. The mapping
also tracks parent-data variants, arity, and current status.

## Legend

| Status | Meaning |
|---|---|
| ✅ | Implemented in `crates/flui-rendering/src/objects/`, tests green. |
| 🚧 | Partial / scaffolded but missing functionality. |
| ⬜ | Not yet implemented. |

## Render objects by family

### Box layout — constraint modifiers (Wave 1, Core.2)

| Render object | Widget(s) | Arity | Parent data | Status | File |
|---|---|---|---|---|---|
| `RenderConstrainedBox` | `Container.constraints`, `ConstrainedBox` | Single | `BoxParentData` | ✅ | `objects/constrained_box.rs` |
| `RenderLimitedBox` | `LimitedBox` | Single | `BoxParentData` | ✅ | `objects/limited_box.rs` |
| `RenderAspectRatio` | `AspectRatio`, video tiles, `Card` cover | Single | `BoxParentData` | ✅ | `objects/aspect_ratio.rs` |
| `RenderFractionallySizedBox` | `FractionallySizedBox`, sheet snap-points | Single | `BoxParentData` | ✅ | `objects/fractionally_sized_box.rs` |

### Box layout — existing (pre-Wave 1)

| Render object | Widget(s) | Arity | Parent data | Status | File |
|---|---|---|---|---|---|
| `RenderPadding` | `Padding`, `Container.padding` | Single | `BoxParentData` | ✅ | `objects/padding.rs` |
| `RenderCenter` | `Center`, `Align(center)` | Single | `BoxParentData` | ✅ | `objects/center.rs` |
| `RenderSizedBox` | `SizedBox` | Leaf | `BoxParentData` | ✅ | `objects/sized_box.rs` |
| `RenderColoredBox` | `ColoredBox`, `Container.color` | Leaf | `BoxParentData` | ✅ | `objects/colored_box.rs` |
| `RenderOpacity` | `Opacity` | Single | `BoxParentData` | ✅ | `objects/opacity.rs` |
| `RenderTransform` | `Transform`, `RotatedBox` | Single | `BoxParentData` | ✅ | `objects/transform.rs` |
| `RenderFlex` | `Row`, `Column`, `Flex` | Variable | `FlexParentData` | ✅ | `objects/flex.rs` |

### Box layout — outstanding (future Core.2 waves)

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderStack` | `Stack`, `IndexedStack`, `Positioned` | Variable | `StackParentData` | ⬜ | parent-data is wired |
| `RenderPositioned` | `Positioned` (within `Stack`) | n/a | `StackParentData` (decorator) | ⬜ | model as ParentDataWidget |
| `RenderWrap` | `Wrap` | Variable | `WrapParentData` (alias of `ContainerBoxParentData`) | ⬜ | |
| `RenderFlow` | `Flow` | Variable | `FlowParentData` | ⬜ | requires `FlowDelegate` (gated) |
| `RenderTable` | `Table` | Variable | `TableCellParentData` | ⬜ | |
| `RenderListBody` | `ListBody` | Variable | `ListBodyParentData` (alias) | ⬜ | |
| `RenderBaseline` | `Baseline` | Single | `BoxParentData` | ⬜ | |
| `RenderIntrinsicWidth` | `IntrinsicWidth` | Single | `BoxParentData` | ⬜ | needs intrinsic plumbing |
| `RenderIntrinsicHeight` | `IntrinsicHeight` | Single | `BoxParentData` | ⬜ | needs intrinsic plumbing |
| `RenderShiftedBox` (base trait) | — | — | — | ⬜ | shared logic for `Padding`/`Align`/etc. — currently inlined per type |

### Paint effects — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderClipRect` | `ClipRect` | Single | `BoxParentData` | ⬜ | candidate for generic `RenderClip<S: ClipShape>` |
| `RenderClipRRect` | `ClipRRect`, rounded `Card` | Single | `BoxParentData` | ⬜ | as above |
| `RenderClipOval` | `ClipOval` | Single | `BoxParentData` | ⬜ | as above |
| `RenderClipPath` | `ClipPath` | Single | `BoxParentData` | ⬜ | as above |
| `RenderDecoratedBox` | `DecoratedBox`, `Container.decoration` | Single | `BoxParentData` | ⬜ | needs `BoxDecoration` painting |
| `RenderRepaintBoundary` | `RepaintBoundary` | Single | `BoxParentData` | ⬜ | needs layer integration |
| `RenderBackdropFilter` | `BackdropFilter` | Single | `BoxParentData` | ⬜ | needs blur/filter pipeline |
| `RenderShaderMask` | `ShaderMask` | Single | `BoxParentData` | ⬜ | needs shader pipeline |
| `RenderCustomPaint` | `CustomPaint` | Single | `BoxParentData` | ⬜ | needs `CustomPainter` (gated) |
| `RenderFittedBox` | `FittedBox` | Single | `BoxParentData` | ⬜ | uses `BoxFit` |
| `RenderFractionalTranslation` | `FractionalTranslation` | Single | `BoxParentData` | ⬜ | |
| `RenderOffstage` | `Offstage` | Single | `BoxParentData` | ⬜ | trivial |
| `RenderAbsorbPointer` | `AbsorbPointer` | Single | `BoxParentData` | ⬜ | hit-test only |
| `RenderIgnorePointer` | `IgnorePointer` | Single | `BoxParentData` | ⬜ | hit-test only |
| `RenderMouseRegion` | `MouseRegion` | Single | `BoxParentData` | ⬜ | needs mouse tracker |
| `RenderPointerListener` | `Listener` | Single | `BoxParentData` | ⬜ | |
| `RenderMetaData` | `MetaData` | Single | `BoxParentData` | ⬜ | trivial |

### Leaf renderers — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderParagraph` | `Text`, `RichText`, `DefaultTextStyle` | Leaf | `TextParentData` | ⬜ | top priority for Core.1 slice |
| `RenderImage` | `Image`, `Card.cover`, `Avatar` | Leaf | `BoxParentData` | ⬜ | needs `flui-assets` re-enable |
| `RenderErrorBox` | error boundary | Leaf | `BoxParentData` | ⬜ | |
| `RenderPerformanceOverlay` | devtools overlay | Leaf | `BoxParentData` | ⬜ | DX track |

### Sliver protocol — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderViewport` | `Viewport`, `Scrollable` body | Variable | `SliverPhysicalParentData` | ⬜ | sliver protocol wired |
| `RenderSliverList` | `SliverList`, `ListView.builder` | Variable | `SliverMultiBoxAdaptorParentData` | ⬜ | |
| `RenderSliverGrid` | `SliverGrid`, `GridView` | Variable | `SliverGridParentData` | ⬜ | needs `SliverGridDelegate` (gated) |
| `RenderSliverPadding` | `SliverPadding` | Single | `SliverPhysicalParentData` | ⬜ | |
| `RenderSliverToBoxAdapter` | `SliverToBoxAdapter` | Single | `SliverPhysicalParentData` | ⬜ | |
| `RenderSliverFillViewport` | `SliverFillViewport`, `PageView` | Variable | `SliverMultiBoxAdaptorParentData` | ⬜ | |
| `RenderSliverFixedExtentList` | `SliverFixedExtentList` | Variable | `SliverMultiBoxAdaptorParentData` | ⬜ | |
| `RenderSliverFillRemaining` | `SliverFillRemaining` | Single | `SliverPhysicalParentData` | ⬜ | |
| `RenderSliverPersistentHeader` | `SliverPersistentHeader`, `SliverAppBar` | Single | `SliverPhysicalParentData` | ⬜ | |

### Semantics — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderSemanticsAnnotations` | `Semantics` | Single | `BoxParentData` | ⬜ | semantics owner already wired |
| `RenderBlockSemantics` | `BlockSemantics` | Single | `BoxParentData` | ⬜ | |
| `RenderExcludeSemantics` | `ExcludeSemantics` | Single | `BoxParentData` | ⬜ | |
| `RenderIndexedSemantics` | `IndexedSemantics` | Single | `BoxParentData` | ⬜ | |
| `RenderMergeSemantics` | `MergeSemantics` | Single | `BoxParentData` | ⬜ | |

## Coverage summary

* **Render objects implemented:** 11 (was 7 before Wave 1, now 11)
* **Render objects planned:** ~80 (Flutter's `rendering/` catalog)
* **Coverage:** **~14%** of the planned catalog (was ~9%)
* **Core.1 vertical-slice unblocked widgets:** `ConstrainedBox`,
  `LimitedBox`, `AspectRatio`, `FractionallySizedBox`, and
  `Container.constraints` (the constraints leg).

## Wave plan

The catalog will be built in **independently parallelizable waves**
(per ROADMAP "Core.2 builds, grouped into arity-correct, independently
parallelizable families"). Each wave ships as a self-contained PR with
tests.

| Wave | Family | Render objects |
|---|---|---|
| **1 (done)** | Constraint modifiers | `RenderConstrainedBox`, `RenderLimitedBox`, `RenderAspectRatio`, `RenderFractionallySizedBox` |
| 2 | Multi-child layout | `RenderStack`, `RenderWrap`, `RenderTable`, `RenderListBody` |
| 3 | Clip + decoration | `RenderClipRect`, `RenderClipRRect`, `RenderClipOval`, `RenderClipPath`, `RenderDecoratedBox`, `RenderRepaintBoundary` |
| 4 | Pointer / mouse + simple proxy | `RenderMouseRegion`, `RenderPointerListener`, `RenderAbsorbPointer`, `RenderIgnorePointer`, `RenderOffstage`, `RenderMetaData`, `RenderFittedBox`, `RenderFractionalTranslation` |
| 5 | Slivers (viewport baseline) | `RenderViewport`, `RenderSliverList`, `RenderSliverToBoxAdapter`, `RenderSliverPadding`, `RenderSliverFillViewport` |
| 6 | Slivers (extended) | `RenderSliverGrid`, `RenderSliverFixedExtentList`, `RenderSliverFillRemaining`, `RenderSliverPersistentHeader` |
| 7 | Text + image leaf | `RenderParagraph`, `RenderImage` (gates Core.1 vertical slice text/image needs) |
| 8 | Semantics annotations | `RenderSemanticsAnnotations`, `RenderBlockSemantics`, `RenderExcludeSemantics`, `RenderIndexedSemantics`, `RenderMergeSemantics` |
| 9 | Filters + custom paint | `RenderBackdropFilter`, `RenderShaderMask`, `RenderCustomPaint`, `RenderBaseline`, `RenderIntrinsicWidth`/`Height` |

Waves 2, 3, 4 can run in parallel (no shared files). Wave 5 must
precede Wave 6 (Wave 6 depends on viewport infra from Wave 5). Wave 7
is on the Core.1 critical path.
