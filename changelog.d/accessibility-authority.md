### Removed
- Remove the unused `flui_semantics::AccessibilityFeatures` type and its
  `accessibility` module. Host-owned `flui_platform_api::SystemPreferences`
  represents observed system preferences; unavailable values remain unknown.
  Removing the legacy flags does not add new preference consumers.
