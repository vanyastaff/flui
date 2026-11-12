### Changed

- **`flui-native`** builds on the published AccessKit adapters (`accesskit_windows`,
  `accesskit_ios`, `accesskit_android`), so it can be published to crates.io. What those releases
  lack is refused, not patched in: iOS and Android report the screen reader's cursor as
  `AssistiveFocusSupport::Unreported`, Windows has no UI Automation `ScrollPattern`, and Android
  publishes no `viewIdResourceName` ([Accessibility bridge fidelity](/docs/gaps.md#accessibility-bridge-fidelity-properties-and-announcements)).
