### Added
- On macOS the host reports high contrast, from `accessibilityDisplayShouldIncreaseContrast`, and the preferred languages, from `NSLocale.preferredLanguages`.

### Changed
- The macOS host no longer samples preferences every 500 ms. It wakes its owner on the workspace's accessibility display-options notification, the current-locale notification and the application becoming active, which is when a double-click interval changed in System Settings is read again.
