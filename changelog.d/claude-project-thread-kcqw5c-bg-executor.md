### Removed

- **`flui-native`**: `Platform::background_executor` and the `PlatformExecutor` trait (also exported
  as `flui_native::PlatformExecutor`) are gone, together with `IOSExecutor` and the per-host
  implementations. Nothing outside the hosts called them; background work goes through the
  capability of ADR-0272. `BackgroundExecutor` stays as the Linux and Windows hosts' own
  marshaling runtime.
