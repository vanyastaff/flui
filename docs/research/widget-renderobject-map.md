# Widget → Render-Object Mapping Checklist

> **Status:** seeded by Core.2 Wave 1 (constraint family), extended
> by Wave 3a (clip family), Wave 2a (`RenderStack`), Wave 4
> (pointer/proxy family), and Wave 5a (sliver proxy family —
> the first production `RenderSliver` impls). Subsequent waves
> extend this table. Per `docs/ROADMAP.md` Core.0 exit criteria,
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

### Multi-child layout — Wave 2a (Core.2)

| Render object | Widget(s) | Arity | Parent data | Status | File |
|---|---|---|---|---|---|
| `RenderStack` | `Stack`, `IndexedStack` | Variable | `StackParentData` | ✅ | `objects/stack.rs` |
| `PositionedSpec` (helper) | typed view that `Positioned` widget builds | — | `StackParentData` reader | ✅ | `objects/stack.rs` |

### Box layout — outstanding (future Core.2 waves)

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderPositioned` | `Positioned` (within `Stack`) | n/a | `StackParentData` (decorator) | ⬜ | model as ParentDataWidget over `StackParentData`; `PositionedSpec` already provides the typed view |
| `RenderWrap` | `Wrap` | Variable | `WrapParentData` (alias of `ContainerBoxParentData`) | ⬜ | |
| `RenderFlow` | `Flow` | Variable | `FlowParentData` | ⬜ | requires `FlowDelegate` (gated) |
| `RenderTable` | `Table` | Variable | `TableCellParentData` | ⬜ | |
| `RenderListBody` | `ListBody` | Variable | `ListBodyParentData` (alias) | ⬜ | |
| `RenderBaseline` | `Baseline` | Single | `BoxParentData` | ⬜ | |
| `RenderIntrinsicWidth` | `IntrinsicWidth` | Single | `BoxParentData` | ⬜ | needs intrinsic plumbing |
| `RenderIntrinsicHeight` | `IntrinsicHeight` | Single | `BoxParentData` | ⬜ | needs intrinsic plumbing |
| `RenderShiftedBox` (base trait) | — | — | — | ⬜ | shared logic for `Padding`/`Align`/etc. — currently inlined per type |

### Paint effects — clip family (Wave 3a, Core.2)

All four clip render objects share **one generic implementation** —
`RenderClip<S: ClipGeometry>` collapses Flutter's 4-class private
`_RenderCustomClip<T>` hierarchy into a single monomorphisable type.

| Render object | Widget(s) | Arity | Parent data | Status | File |
|---|---|---|---|---|---|
| `RenderClipRect` | `ClipRect`, `Card` (non-rounded) | Single | `BoxParentData` | ✅ | `objects/clip.rs` |
| `RenderClipRRect` | `ClipRRect`, rounded `Card`, `Chip` | Single | `BoxParentData` | ✅ | `objects/clip.rs` |
| `RenderClipOval` | `ClipOval`, `CircleAvatar` | Single | `BoxParentData` | ✅ | `objects/clip.rs` |
| `RenderClipPath` | `ClipPath`, custom-shaped surfaces | Single | `BoxParentData` | ✅ | `objects/clip.rs` |

### Paint effects — pointer / visibility / transform proxy (Wave 4, Core.2)

| Render object | Widget(s) | Arity | Parent data | Status | File |
|---|---|---|---|---|---|
| `RenderOffstage` | `Offstage` | Single | `BoxParentData` | ✅ | `objects/offstage.rs` |
| `RenderAbsorbPointer` | `AbsorbPointer` | Single | `BoxParentData` | ✅ | `objects/absorb_pointer.rs` |
| `RenderIgnorePointer` | `IgnorePointer` | Single | `BoxParentData` | ✅ | `objects/ignore_pointer.rs` |
| `RenderMetaData` | `MetaData` | Single | `BoxParentData` | ✅ | `objects/meta_data.rs` |
| `RenderFractionalTranslation` | `FractionalTranslation` | Single | `BoxParentData` | ✅ | `objects/fractional_translation.rs` |
| `RenderFittedBox` | `FittedBox` | Single | `BoxParentData` | ✅ | `objects/fitted_box.rs` |

### Paint effects — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderDecoratedBox` | `DecoratedBox`, `Container.decoration` | Single | `BoxParentData` | ⬜ | needs `BoxDecoration` painting (Wave 3b) |
| `RenderRepaintBoundary` | `RepaintBoundary` | Single | `BoxParentData` | ⬜ | needs layer integration (Wave 3b) |
| `RenderBackdropFilter` | `BackdropFilter` | Single | `BoxParentData` | ⬜ | needs blur/filter pipeline (Wave 9) |
| `RenderShaderMask` | `ShaderMask` | Single | `BoxParentData` | ⬜ | needs shader pipeline (Wave 9) |
| `RenderCustomPaint` | `CustomPaint` | Single | `BoxParentData` | ⬜ | needs `CustomPainter` (gated, Wave 9) |
| `RenderMouseRegion` | `MouseRegion` | Single | `BoxParentData` | ⬜ | needs mouse tracker (Wave 4b) |
| `RenderPointerListener` | `Listener` | Single | `BoxParentData` | ⬜ | needs pointer-event routing (Wave 4b) |

### Leaf renderers — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderParagraph` | `Text`, `RichText`, `DefaultTextStyle` | Leaf | `TextParentData` | ⬜ | top priority for Core.1 slice |
| `RenderImage` | `Image`, `Card.cover`, `Avatar` | Leaf | `BoxParentData` | ⬜ | needs `flui-assets` re-enable |
| `RenderErrorBox` | error boundary | Leaf | `BoxParentData` | ⬜ | |
| `RenderPerformanceOverlay` | devtools overlay | Leaf | `BoxParentData` | ⬜ | DX track |

### Sliver protocol — proxy family (Wave 5a, Core.2)

First production `RenderSliver` impls. All Single arity,
`SliverPhysicalParentData`. Pure Sliver→Sliver passthroughs that
establish the convention for the sliver catalog.

| Render object | Widget(s) | Arity | Parent data | Status | File |
|---|---|---|---|---|---|
| `RenderSliverPadding` | `SliverPadding` | Single | `SliverPhysicalParentData` | ✅ | `objects/sliver_padding.rs` |
| `RenderSliverOpacity` | `SliverOpacity` | Single | `SliverPhysicalParentData` | ✅ | `objects/sliver_opacity.rs` |
| `RenderSliverIgnorePointer` | `SliverIgnorePointer` | Single | `SliverPhysicalParentData` | ✅ | `objects/sliver_ignore_pointer.rs` |
| `RenderSliverOffstage` | `SliverOffstage` | Single | `SliverPhysicalParentData` | ✅ | `objects/sliver_offstage.rs` |

### Sliver protocol — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderViewport` | `Viewport`, `Scrollable` body | Variable | `SliverPhysicalParentData` | ⬜ | the Box↔Sliver bridge — Wave 5b |
| `RenderSliverToBoxAdapter` | `SliverToBoxAdapter` | Single | `SliverPhysicalParentData` | ⬜ | the Sliver↔Box bridge — Wave 5b |
| `RenderSliverList` | `SliverList`, `ListView.builder` | Variable | `SliverMultiBoxAdaptorParentData` | ⬜ | lazy children — Wave 5c |
| `RenderSliverFillViewport` | `SliverFillViewport`, `PageView` | Variable | `SliverMultiBoxAdaptorParentData` | ⬜ | Wave 5c |
| `RenderSliverFixedExtentList` | `SliverFixedExtentList` | Variable | `SliverMultiBoxAdaptorParentData` | ⬜ | Wave 5c |
| `RenderSliverFillRemaining` | `SliverFillRemaining` | Single | `SliverPhysicalParentData` | ⬜ | Wave 5b |
| `RenderSliverGrid` | `SliverGrid`, `GridView` | Variable | `SliverGridParentData` | ⬜ | needs `SliverGridDelegate` (gated) — Wave 6 |
| `RenderSliverPersistentHeader` | `SliverPersistentHeader`, `SliverAppBar` | Single | `SliverPhysicalParentData` | ⬜ | Wave 6 |

### Semantics — outstanding

| Render object | Widget(s) | Arity | Parent data | Status | Notes |
|---|---|---|---|---|---|
| `RenderSemanticsAnnotations` | `Semantics` | Single | `BoxParentData` | ⬜ | semantics owner already wired |
| `RenderBlockSemantics` | `BlockSemantics` | Single | `BoxParentData` | ⬜ | |
| `RenderExcludeSemantics` | `ExcludeSemantics` | Single | `BoxParentData` | ⬜ | |
| `RenderIndexedSemantics` | `IndexedSemantics` | Single | `BoxParentData` | ⬜ | |
| `RenderMergeSemantics` | `MergeSemantics` | Single | `BoxParentData` | ⬜ | |

## Coverage summary

* **Render objects implemented:** **26** (7 → 11 → 15 → 16 → 22 → 26
  across Wave 1 → Wave 3a → Wave 2a → Wave 4 → Wave 5a). The 4 clip
  variants share **one generic implementation** — monomorphisable,
  no vtables in paint/hit-test. Wave 5a adds the first production
  `RenderSliver` impls (sliver-side proxy family).
* **Render objects planned:** ~80 (Flutter's `rendering/` catalog).
* **Coverage:** **~32.5%** of the planned catalog (was ~27.5%).
* **Core.1 vertical-slice unblocked widgets:** `ConstrainedBox`,
  `LimitedBox`, `AspectRatio`, `FractionallySizedBox`,
  `Container.constraints`, the full `ClipRect` / `ClipRRect` /
  `ClipOval` / `ClipPath` family, `Stack` + `Positioned`, the
  Wave 4 pointer/proxy family (`Offstage`, `AbsorbPointer`,
  `IgnorePointer`, `MetaData`, `FractionalTranslation`,
  `FittedBox`), plus Wave 5a sliver proxies: `SliverPadding`,
  `SliverOpacity`, `SliverIgnorePointer`, `SliverOffstage`.

## Wave plan

The catalog will be built in **independently parallelizable waves**
(per ROADMAP "Core.2 builds, grouped into arity-correct, independently
parallelizable families"). Each wave ships as a self-contained PR with
tests.

| Wave | Family | Render objects |
|---|---|---|
| **1 (done)** | Constraint modifiers | `RenderConstrainedBox`, `RenderLimitedBox`, `RenderAspectRatio`, `RenderFractionallySizedBox` |
| **2a (done)** | Multi-child overlay | `RenderStack` + `PositionedSpec` typed view |
| 2b | Multi-child layout (remaining) | `RenderWrap`, `RenderTable`, `RenderListBody` |
| **3a (done)** | Clip family (generic) | `RenderClip<S: ClipGeometry>` + `RenderClipRect` / `RRect` / `Oval` / `Path` aliases + `Oval` newtype |
| 3b | Decoration + repaint boundary | `RenderDecoratedBox`, `RenderRepaintBoundary` |
| **4 (done)** | Pointer / visibility / transform proxy | `RenderOffstage`, `RenderAbsorbPointer`, `RenderIgnorePointer`, `RenderMetaData` (+ `MetaDataPayload`), `RenderFractionalTranslation` (+ `TranslationFraction`), `RenderFittedBox` |
| 4b | Mouse / pointer events | `RenderMouseRegion`, `RenderPointerListener` (need mouse-tracker + pointer-event routing infra) |
| **5a (done)** | Sliver proxy family | `RenderSliverPadding`, `RenderSliverOpacity`, `RenderSliverIgnorePointer`, `RenderSliverOffstage` — first production `RenderSliver` impls |
| 5b | Sliver bridges | `RenderViewport` (Box→Sliver bridge), `RenderSliverToBoxAdapter` (Sliver→Box bridge), `RenderSliverFillRemaining` |
| 5c | Sliver lists | `RenderSliverList`, `RenderSliverFixedExtentList`, `RenderSliverFillViewport` (lazy children) |
| 6 | Slivers (extended) | `RenderSliverGrid`, `RenderSliverPersistentHeader` |
| 7 | Text + image leaf | `RenderParagraph`, `RenderImage` (gates Core.1 vertical slice text/image needs) |
| 8 | Semantics annotations | `RenderSemanticsAnnotations`, `RenderBlockSemantics`, `RenderExcludeSemantics`, `RenderIndexedSemantics`, `RenderMergeSemantics` |
| 9 | Filters + custom paint | `RenderBackdropFilter`, `RenderShaderMask`, `RenderCustomPaint`, `RenderBaseline`, `RenderIntrinsicWidth`/`Height` |

Waves 2, 3, 4 can run in parallel (no shared files). Wave 5 must
precede Wave 6 (Wave 6 depends on viewport infra from Wave 5). Wave 7
is on the Core.1 critical path.
