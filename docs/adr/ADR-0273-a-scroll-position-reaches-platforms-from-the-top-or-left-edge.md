# ADR-0273: A scroll position reaches platforms from the top or left edge

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0157](ADR-0157-native-scroll-range-actions.md) §Decision, second paragraph
  only (the range carries a foundation `Axis`)
- **Related:** [ADR-0206](ADR-0206-typed-semantics-attributes.md) (the scroll group of
  `SemanticsAttributes`), [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (decision 18, the Web's parallel ARIA
  DOM, which places a scroller by this position)

## Context and problem statement

A `Scrollable` publishes its `ScrollPosition`'s pixels as the node's scroll position, with the
extents around it (`flui-objects` `RenderSemanticsAnnotations::describe_semantics_configuration`).
Pixels grow in the scrollable's `AxisDirection`: for a list that grows up (a chat) or a horizontal
one in a right-to-left locale, pixel 0 is the bottom or right edge. The semantics carried only
the `Axis`, so nothing downstream could tell which edge the position counts from. A platform that
reads a position counts from the top or left edge: a browser's `scrollTop` and `scrollLeft`, and
UI Automation's scroll percent (0 % is the top). The Web's semantics host makes a scrollable node's
element a real scroller, and cannot place a reversed list's content or turn its `scroll` event
back into an offset. And `Scrollable` took no `SetScrollOffset`, so an offset a platform asks for
had nowhere to go. Which order does a published scroll position use, and who converts it?

## Decision drivers

- One value per fact: an axis and a reversal flag can contradict each other, an `AxisDirection`
  cannot.
- The owner's reveal (`SemanticsOwner`, `Reveal::scroll_position`) compares the sampled position
  with the controller's pixels, so the configuration must keep pixels.
- Platforms read one order; the conversion is written once, not once per adapter.
- `flui-semantics` depends on `flui-foundation` only, so the direction type lives there.

## Considered options

1. The configuration carries the `AxisDirection`; the AccessKit translation publishes the position
   from the top or left edge; `SetScrollOffset` takes that order and the scrollable converts back.
2. Keep the `Axis` and pixels; the Web host keeps its scroller at a neutral offset and turns each
   `scroll` event into one directional page step.
3. Publish pixels in the configuration already mirrored.

## Decision

Chosen option: 1, because it gives every platform the order it reads with one conversion each way
and keeps pixels where FLUI compares them.

- `AxisDirection` moves to `flui_foundation::geometry`; `flui_rendering::constraints` re-exports it.
- `SemanticsConfiguration::set_scroll_axis_direction(AxisDirection)` and `scroll_axis_direction()`
  replace `set_scroll_axis(Axis)` and `scroll_axis()`. Without one, a range grows from top to
  bottom, as the vertical fallback did. The position and extents stay pixels.
- The AccessKit translation publishes `scroll_x`/`scroll_y` on the direction's axis. A reversed
  direction publishes `min + max − pixels`; with either extent unknown or the result not finite, the
  position is left out (an unbounded list has no top-edge place), the extents are still published.
- `ActionCall::SetScrollOffset` is in the published order. `Scrollable` handles it on its viewport
  node: the component on its axis, mirrored with the position's current extents when reversed, then
  the same user-input movement as every other accessibility scroll. A non-finite result moves
  nothing.
- The scrollbar's numeric adjustment (`SetNumericValue` on the position slider) keeps pixels: it is a
  value, not a position, and its range is published as such.

### Consequences

- Good, because a reversed list scrolls and reads the same as any other on every adapter that reads
  a position, and the Web host places it with no knowledge of FLUI's pixels.
- Good, because a platform can scroll to an exact offset: a screen reader's swipe or a browser
  revealing its cursor's element.
- Bad, because the published position and the configuration's differ for a reversed scrollable,
  so a reader of the AccessKit tree and a reader of the configuration see different numbers.
- Neutral, because `accesskit_windows` 0.35 implements no `ScrollPattern` yet: UI Automation reads
  the position once it does.

### Confirmation

- The compiler: the configuration takes an `AxisDirection`, no separate flag exists.
- `flui-semantics`'s AccessKit coverage matrix rows "a list growing up is published from the top
  edge", "a right-to-left scroll is published from the left edge" and "a reversed scroll with an
  unbounded extent has no position".
- `scroll_offsets_reach_platforms_from_the_top_or_left_edge` in `flui-widgets` (both axes, both
  orders): the published position and a `SetScrollOffset` request through the mounted widget's
  AccessKit tree.

## Pros and cons of the options

### 1. The direction in the configuration, the top-edge order at the platform

- Good, because both conversions sit beside the code that owns each side.
- Bad, because `SetScrollOffset` handlers must know their direction.

### 2. A neutral offset and directional steps

- Good, because no API changes.
- Bad, because a reveal or a short swipe becomes a page step: the content jumps past the element the
  reader moved to.

### 3. Mirrored pixels in the configuration

- Good, because the translation stays a copy.
- Bad, because the owner's reveal and every other reader of the configuration would compare a
  mirrored position with the controller's pixels.

## More information

- AccessKit 0.25 documents `scroll_x`/`scroll_y` without an origin; the top-left origin is what its
  consumers' platforms read (`Element.scrollTop`, UI Automation `IScrollProvider`).
- Revisit when an adapter reads a position from another edge.
