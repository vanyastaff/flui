### Added

- `GestureDetector::on_smart_zoom` reports a trackpad smart zoom (a two-finger double tap on macOS) as `SmartZoomDetails` with its `SmartZoomSource`; only the innermost detector under the pointer receives it.
- `PointerEvent::SmartZoom(SmartZoomEvent)` carries the platform's smart zoom gesture; AppKit's `smartMagnifyWithEvent:` produces it, and `Listener::on_smart_zoom_claim` and `RenderObject::smart_zoom_target` take part in the leaf-first claim like scroll and pan-zoom.
- `TrackpadScript::smart_zoom` replays a smart zoom in widget tests, and recordings round-trip it.
