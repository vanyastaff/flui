### Changed

- **`flui-runtime`**: the realm core moved here from `flui-app` (`UiRealm`, its presentations
  and their frame transaction, lifecycle, frame-failure reporting and `RenderingFlutterBinding`;
  [ADR-0083](/docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) move 4). The realm
  renders through any `FrameSink` and names no engine type. The `flui_app` paths of
  `FailureDisposition`, `FrameFailureDetail`, `FrameFailureHandler`, `FrameFailureKind`,
  `FrameFailureReport`, `PanicText`, `SegmentPhase` and `bindings::RenderingFlutterBinding` are
  unchanged.
- **`flui-semantics`**: `PlatformAccessibility` and its listener aliases are defined in
  `flui_semantics::platform`; `flui_platform::traits` re-exports them at their old paths
  ([ADR-0082](/docs/adr/ADR-0082-platform-api-contract-crate.md) §2, amended).
- **`flui-foundation`**: `diagnostics::REDACTED_VALUE` is the redaction placeholder;
  `flui_log::REDACTED_VALUE` re-exports it.
- **`flui-app`**: closing one presentation of a realm no longer hides that presentation's
  `GlobalKey`s from the realm-wide lookup while it detaches; they resolve until its tree is torn
  down, and resolve to nothing during the teardown, as before.
