### Added

- `PlatformInput::Capabilities` carries a window's handle-less input capabilities
  (`flui_platform::input_capabilities`: `InputCapabilities`, `Availability`,
  `PointerKindSet`): a snapshot before any other input once `on_input` is
  installed, then each change in order with the input around it (ADR-0213). The
  SDK and the facade re-export them behind the additive `unstable-input` feature.
- `PointerEvent::Revise` (final sensor values for a delivered reading, delivered to
  `Listener::on_pointer_revise` and never to recognizers), `PointerEvent::PenSqueeze`,
  `PointerSample::hover_distance` and `PointerKind::Virtual`.
- The headless backend attaches and removes simulated devices through
  `HeadlessPlatform::input_devices` and states its availability with
  `HeadlessPlatform::with_input_availability`.

### Changed

- `PanZoomTransform` carries only scale and rotation; translation during a pinch is
  the focal point (`PanZoomEvent::position`), refreshed on every update. A two-finger
  trackpad pan is a phased `Scroll` on every backend, winit's `PanGesture` included
  (ADR-0217 §4).
- `TransferActions`' `NONE` constant is `empty()`.
- Input admitted before a window's input callback is installed is held and delivered
  after the capability snapshot instead of being dropped.

### Removed

- `PlatformCapabilities`, `DesktopCapabilities`, `MobileCapabilities`,
  `WebCapabilities`, `Platform::capabilities` and `OwnerPlatform::capabilities`
  (ADR-0213 §4).
