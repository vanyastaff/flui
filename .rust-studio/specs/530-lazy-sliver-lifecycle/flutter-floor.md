# Flutter floor for #530 (read from .flutter @ 09aafef, 2026-08-06; packages/flutter/lib/src)

## Child manager contract (rendering/sliver_multi_box_adaptor.dart:25-136)
- createChild(index, after) — element builds the child inside owner.buildScope; may produce nothing (end of list).
- removeChild(child) — element deactivates; render remove happens via removeRenderObjectChild.
- estimateMaxScrollOffset(constraints, first, last, leading, trailing) — element extrapolates from average extent
  (widgets/sliver.dart _extrapolateMaxScrollOffset) unless delegate gives childCount (finite) → `childCount` getter
  otherwise does an open-ended binary search calling build(hi-1) until null (!).
- childCount / estimatedChildCount.
- didAdoptChild(child) — stamps parentData.index = _currentlyUpdatingChildIndex. **Index is stamped at render adoption,
  not at view creation** — slot is carried through ComponentElements, so composite children (Text/StatefulWidget at the
  top) get stamped when their first render descendant attaches (insertRenderObjectChild(child, slot)).
- setDidUnderflow(bool) — layout hit the end; performRebuild uses it to look one child past the last key.
- didStartLayout/didFinishLayout — didFinishLayout(firstIndex,lastIndex) forwarded to delegate.
- debugAssertChildListLocked — child list may only change inside createChild/removeChild (i.e. via invokeLayoutCallback).

## Adaptor render base (RenderSliverMultiBoxAdaptor)
- Children are a contiguous doubly-linked list ordered by index (assert debugAssertChildListIsNonEmptyAndContiguous).
- _keepAliveBucket: Map<int, RenderBox>; keepAlive children leave the list but stay attached (attach/detach/redepth/visitChildren
  include bucket; visitChildrenForSemantics EXCLUDES bucket). Only `_createOrObtainChild` re-inserts from bucket.
- addInitialChild(index, layoutOffset) / insertAndLayoutLeadingChild / insertAndLayoutChild — every creation is inside
  invokeLayoutCallback (build during layout), returns null + setDidUnderflow(true) when the manager produced nothing.
- calculateLeadingGarbage(firstIndex) / calculateTrailingGarbage(lastIndex) / collectGarbage(l, t) — destroy or cache;
  then removeChild for bucket entries whose keepAlive flipped false.
- paint: iterate children; paint child only if (mainAxisDelta < remainingPaintExtent && mainAxisDelta + extent > 0).
- hitTestChildren: walks from lastChild backwards using childMainAxisPosition; hitTestBoxChild.
- semanticBounds: when !geometry.visible but firstChild has size → report firstChild.paintBounds (so AT can reach).

## Element (widgets/sliver.dart:929-1307 SliverMultiBoxAdaptorElement)
- _childElements: SplayTreeMap<int, Element?>. performRebuild: rebuild every resident index; keys: for each resident child
  with a key, delegate.findIndexByKey(key) → if moved, remap (layoutOffset := null on the moved child), optionally
  _replaceMovedChildren. Preserves layoutOffset across render-object swap (updateChild override). After rebuild, if nothing
  changed && _didUnderflow → also process lastKey+1 (so max scroll extent can grow without a layout pass).
- createChild/removeChild wrap in owner.buildScope(this, ...); _currentlyUpdatingChildIndex set around updateChild.
- forgetChild(child) removes by slot (GlobalKey reparenting path).
- debugVisitOnstageChildren: children whose [layoutOffset, layoutOffset+extent) intersects [scrollOffset, scrollOffset+remainingPaintExtent).

## Delegate (widgets/scroll_delegate.dart:352-580 SliverChildBuilderDelegate)
- build(ctx, index): index range check → try builder catch → _createErrorWidget(exception, stack) (FlutterError.reportError +
  ErrorWidget.builder). Wrap order: RepaintBoundary (addRepaintBoundaries) → IndexedSemantics (addSemanticIndexes) →
  AutomaticKeepAlive(_SelectionKeepAlive) (addAutomaticKeepAlives) → KeyedSubtree(key: _SaltedValueKey(child.key)).
- findIndexByKey: unwraps _SaltedValueKey then user findChildIndexCallback; List delegate builds _keyToIndexMap lazily.
- shouldRebuild → true for builder delegate.

## Layout algorithms
- RenderSliverList.performLayout (sliver_list.dart:~95-341): variable extents; walks from firstChild outward; corrections:
  (a) ran out of leading children before reaching scrollOffset → SliverGeometry(scrollOffsetCorrection: -scrollOffset);
  (b) firstChildScrollOffset < -tolerance → correction -firstChildScrollOffset, firstChild.layoutOffset = 0;
  (c) scrollOffset≈0 but firstChild.index>0 → insert leading one at a time, correction if extent>0.
  Leading garbage = children ending before scrollOffset (keep last one when list exhausted to know extent);
  trailing = children after target end. estimatedMaxScrollOffset via manager unless reachedEnd. hasVisualOverflow conservative.
  Underflow when estimatedMax == endScrollOffset.
- RenderSliverFixedExtentBoxAdaptor.performLayout (sliver_fixed_extent_list.dart:326-495): index math from extent;
  garbage first (calculateLeading/Trailing), addInitialChild(firstIndex, indexToLayoutOffset); leading insert failure →
  correction = indexToLayoutOffset(index); trailing loop bounded by targetLastIndex; estimatedMax = min(end-of-list offset,
  estimateMaxScrollOffset); paint/cache extents from index math.
- RenderSliverGrid.performLayout (sliver_grid.dart:594-728): layout = delegate.getLayout(constraints); first/last index from
  layout; garbage first; geometry per index (scrollOffset, crossAxisOffset, trailingScrollOffset); trailingScrollOffset = max
  over laid-out children; no correction path (leading insert asserted non-null); paintExtent from min(scrollOffset, leading).
- RenderViewport.performLayout (viewport.dart:1685-1760): loop up to 10*childCount: correction != 0 → offset.correctBy(correction)
  and retry; else applyContentDimensions; throws after max cycles.

## Keep-alive (automatic_keep_alive.dart, KeepAlive ParentDataWidget in sliver.dart)
- KeepAliveNotification bubbles to AutomaticKeepAlive which sets parentData.keepAlive via KeepAlive widget; adaptor's
  _destroyOrCacheChild honors it. Not in #530's acceptance list — record as follow-up unless trivially carried.
