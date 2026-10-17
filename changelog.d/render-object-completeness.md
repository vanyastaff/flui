### Fixed

- Preserve the selected wheel consumer's exact admission after reentrant raw
  observers. Deliver completion after native termination, cancellation or
  owner-clock inactivity, preserving accepted delivery outside the owner lane.
- Honor rich-text accessibility labels and reading-order keys, including local
  replacements; refuse non-finite reading orders before publication.
- Publish lazy collection totals on actual List/Grid containers and preserve
  honest unknown totals for nested and explicitly numbered sets.

- Exclude positioned Stack overlays from intrinsic sizing so intrinsic wrappers
  retain the content size instead of expanding to an overlay's dimensions.
- Cancel animation and viewport subscriptions when their render owner is
  destroyed, including opacity proxy cycles. Preserve viewport replacement,
  stale callback refusal and subsequent progress after subscription failures.
- Preserve live baselines through sizing, pointer and paint wrappers, including
  deeply nested proxies; add positioned Stack/Wrap and onstage Theater baselines.
- Make Baseline and Skeleton dry sizing agree with committed layout, and make
  Baseline's live and dry queries agree on its explicit authored line.
- Include mandatory child gaps in Wrap's maximum intrinsic main extent.
- Refuse unrepresentable positioned baseline sums in Wrap and IndexedStack.
- Remove zero-opacity and offstage sliver children from accessibility,
  including stale action routing after visibility changes.
- Apply physical surfaces' child clip to accessible bounds and republish
  those bounds when clip behavior changes.
- Preserve constrained image proportions and paint at tiny positive asset
  scales without overflowing intermediate logical extents.
- Keep Flow accessibility clipped and limited to presented children, including
  repaint notifications and delegate replacements.
- Complete CustomPaint and Flow subscription cleanup on detach and owner release,
  preserve replacement progress after callback failures, and revoke outgoing or
  unacknowledged listener callbacks before propagating failures.
- Refresh custom-painted semantic regions on repaint notifications, including
  their bounds, labels, action callbacks and removal.
- Preserve semantics draft rejection and retry when rejected callback captures
  have hostile destructors, following exceptional-path retention policy.

### Changed

- `RenderImage` and the `Image` widget take an `ImagePlacement` from
  `flui-painting` instead of `ImageFit`/`ImageAlignment`, which are removed.
  Replace `RenderImage::new(size, ImageFit::Contain, ImageAlignment::Center)`
  with `RenderImage::new(size, ImagePlacement::fitted(BoxFit::Contain,
  Alignment::CENTER))`, and `set_fit`/`set_alignment` with `set_placement`.
  `Image::fit` and `Image::alignment` now take `BoxFit` and `Alignment`.

- `DragUpdateDetails::new` takes the actual global delta after the local delta.
  Drag and multi-drag updates expose that root movement; drag ends expose raw
  root velocity and `global_fling_velocity()` with the admitted fling policy.
- `Listener::on_scroll_claim` receives `ScrollDispatch`, with explicit `local`
  and `global` packets, sequence, source and cause. `on_scroll_sequence` uses
  the same selected target and also receives typed completion. A standalone
  route has no binding sequence; native pan/zoom dispatch now carries one when
  admitted by the binding.

- Lazy render implementors must install declared member totals through
  `LazyMultiBoxRender::set_semantic_set_size`; counts remain separate from slot
  positions and do not depend on resident children.

- `Flow` and `RenderFlow` now take `Rc<dyn FlowDelegate>` because delegates and
  their repaint sources belong to the UI owner. Replace delegate `Arc` handles
  with `Rc` handles.
- `RenderTheater` no longer implements `Eq`, because its layout state now
  includes fractional baseline measurements.
- The render-object `visits_child_for_semantics` hook now receives committed
  `Size` as its second argument. Existing implementations can add `_size`.
- Replace the inert `SemanticsBuilder` and `CustomPainter::semantics_builder`
  with `CustomPainter::semantics_regions(size)`. Return keyed `SemanticsRegion`
  values for independently accessible content, or use the default empty list.

### Added

- Images support every `BoxFit` (including `FitWidth` and `FitHeight`),
  fractional alignment, repetition (`Image::repeat`) and nine-slice
  stretching (`Image::center_slice`) whose corners keep their logical size at
  any image scale. Decoration and render-object images share one placement.

- Binding-owned input sequence identities, retained in arena/contact admission
  and accepted drag-start metadata, with permanent exhaustion refusal.

- `SliverCrossAxisGroup` and `SliverCrossAxisItem` for parallel content-sized
  and weighted tracks, and `SliverConstrainedCrossAxis` for validated limits.

- Vertical-direction configuration on Flex, Row and Column, with consistent
  positioned live/dry baselines and mounted updates.
- Continuous per-child Flow group opacity, including repaint boundaries and
  accessibility/hit-test omission at zero opacity.

- `SliverSemantics` and `RenderSliverSemanticsAnnotations` for protocol-preserving
  annotations, and `Semantics::block_previous_siblings` for accessible overlays.
- Typed authored/lazy collection declarations and membership origins.

- `SliverFadeTransition`, an animation-driven sliver fade with stable render
  identity, source replacement and zero-opacity semantics handling.
- Custom-painter accessible regions with separate bounds, transforms, actions
  and stable keys, ordered around real child semantics. Retained keys survive
  reordering and semantics rebuilds; removed keys cannot receive stale actions.
- Presentation-local synthetic semantics identities, atomic duplicate-key and
  exhaustion refusal, and staged partial semantics updates.

### Removed

- Per-item collection totals: `SliverSlot::set_size`, `with_set_size` and
  `SliverMultiBoxAdaptorParentData::semantic_set_size`. Declare the total through
  the host's effective semantic mapping instead.

- Inert `allow_implicit_scrolling` methods and storage from `ViewportOffset`,
  `ScrollableViewportOffset` and `RenderSliverFillViewport`. Remove getter/setter
  calls and the trait method from external implementations. Descendant reveal
  continues to follow registered semantics ancestors (ADR-0168).
- Inert `RenderCustomPaint` raster-cache hints (`with_is_complex`,
  `with_will_change`, `is_complex`, `will_change`). Remove these calls;
  the hints never influenced rendering or caching.
