### Added

- **`AnchoredDrag`**: a one-dimensional drag of its child that comes to rest on anchors, the base
  of swipe actions, dismissible rows and sheets. `DraggableAnchors::builder().at(key, offset)`
  builds the anchors (`AnchorsError` refuses an empty set, a non-finite, out-of-range or
  coincident offset and a repeated key); `AnchoredDragController` reports the current anchor and
  whether the drag moves, replaces the anchors mid-drag, and animates or jumps to an anchor. A
  release settles on the nearest anchor, a fling on the next one; offsets run toward the reading
  end, so right-to-left text mirrors a horizontal drag. It is a bounded leaf of the nested scroll
  channel, and a participant for the scrollables inside it: a list at its edge hands it the rest
  of the drag, and the list's release settles it. Arriving at a new anchor plays a selection
  haptic with the frame that shows it; assistive increment and decrement and the arrow keys move
  between anchors.

### Changed

- **A `Scrollable` takes a new snap policy in place**: replacing it no longer retires the
  scrollable's place in the scroll chain, so the drag and the settle in progress go on; the next
  release settles through the new policy.
