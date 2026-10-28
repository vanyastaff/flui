### Added

- **`flui-native`**: accessibility on iOS. Every UIKit window answers UIAccessibility through
  `accesskit_ios`'s low-level adapter, which the window's own view forwards
  `isAccessibilityElement`, `accessibilityElements` and `accessibilityHitTest:` to, and a semantics
  identifier reaches `accessibilityIdentifier`; run with an accessibility client on the iOS
  Simulator ([ADR-0255](/docs/adr/ADR-0255-mobile-and-web-hosts.md)).
