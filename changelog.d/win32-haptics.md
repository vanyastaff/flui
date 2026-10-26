### Added

- **`flui-native`**: Win32 windows play haptics on the pen in range of them through its
  `SimpleHapticsController`, both tiers by the Windows degradation table; with no pen in range a
  request reports `Unavailable`
  ([ADR-0210](/docs/adr/ADR-0210-two-tier-haptics.md)). Touchpad and mouse haptics
  (`InputHapticsManager`) stay unavailable.
