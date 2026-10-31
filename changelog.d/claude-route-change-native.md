### Added

- **`flui-native`**: on iOS a route change posts `UIAccessibilityScreenChangedNotification`, and on
  Android it raises `TYPE_WINDOW_STATE_CHANGED` carrying the route's name, so VoiceOver and TalkBack
  treat it as a new screen rather than an announcement
  ([ADR-0204](/docs/adr/ADR-0204-synthetic-semantics-nodes.md) §5).
