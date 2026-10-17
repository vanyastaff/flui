# ADR-0232: Annotated regions and the coalesced system overlay style

- **Status:** Accepted
- **Date:** 2026-10-17
- **Supersedes:** [ADR-0031](ADR-0031-platform-haptics-capability-and-system-chrome-deferral.md) §5, for the
  system-bar overlay style only; orientation, system UI modes and the change callback stay deferred

## Context

`AnnotatedRegionLayer` attached a type-erased value to a rectangle, but nothing
produced it from the render tree and nothing read it. ADR-0031 §5 deferred all of
system chrome and recorded what a port of the overlay style needs: a framework
coalescer with pending/latest deduplication and a cache cleared on detach, fed per
frame by annotated regions rather than by direct calls.

## Decision

**Lookup.** `LayerTree::find_annotations::<T>(point)` returns every `T` at a scene
point, topmost first: later siblings and a region's own children before the region.
The point maps through offset-carrying layers, transforms (a singular one hides its
subtree) and resolved follower offsets; a clip whose shape excludes the point hides
its subtree, and `Clip::None` clips nothing. `find_annotation` takes the first.

**Producer.** `RenderAnnotatedRegion` annotates its own box around its child through
a paint scope; `AnnotatedRegion<T>` is the widget, comparing values so an equal
value on rebuild does not repaint.

**Overlay style.** `flui-platform-api` owns `SystemUiOverlayStyle` and the
`PlatformSystemChrome` capability (`PlatformWindow::system_chrome`, `None` by
default). After each painted frame a presentation probes the middle of its window's
top edge and stages the topmost style; it delivers it after the sink accepts that
frame, and only when it differs from the last delivery, so refused and deferred
frames announce nothing. A frame without the annotation leaves the platform's style
alone. `WindowExecutionState::Detached` forgets the last delivery, so the next frame
after reattaching resends. A window without the capability records no delivery, so a
backend that gains it later still receives the current style.

The headless backend records deliveries. Android applies the style on the Java
main thread: `WindowInsetsController.setSystemBarsAppearance` with
`APPEARANCE_LIGHT_STATUS_BARS` on API 30+, the `SYSTEM_UI_FLAG_LIGHT_STATUS_BAR`
visibility flag on API 23–29, nothing below 23 (the icon colour is not
controllable). iOS installs a `FluiViewController` as each attachment's root view
controller; its `preferredStatusBarStyle` reads the requested style
(`LightContent`, or `DarkContent` from iOS 13 and `Default` before it), and a new
style calls `setNeedsStatusBarAppearanceUpdate` on the main thread. Desktop and Web
backends return no capability.

## Verification

`annotation_lookup` (flui-layer) pins ordering, offsets and transforms, clips,
singular transforms and type filtering. `harness_annotated_region_is_found_in_the_scene_over_its_box`
pins the producer. The `overlay_style` rows of `frame_pacing_and_pump_matrix`
(flui-runtime) pin one delivery per change, no delivery for a refused frame, and
the resend after detach. The Android and iOS implementations are type-checked only;
no device run has verified them.
