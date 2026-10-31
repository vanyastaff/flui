### Changed

- **`flui-app`**: when the first locale of the system preferences changes, the host font feed's
  thread asks the platform again which faces fall back and installs the new order, so text in
  Han and other locale-sensitive scripts follows the new locale
  ([ADR-0271](/docs/adr/ADR-0271-the-host-feed-asks-the-platform-which-faces-fall-back.md) §4).

### Added

- **`flui-painting`**: `FallbackOrder::locale`, the locale of the fallback order installed last.
