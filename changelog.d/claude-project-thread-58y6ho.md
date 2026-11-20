### Added

- **`platform-support.toml`**: the lowest version of each environment FLUI supports, declared
  once, with the generated [version map](/docs/version-map.md)
  ([ADR-0244](/docs/adr/ADR-0244-os-version-floors-and-gates.md)). `flui-cli`'s `platform_floor` test
  fails when a generated floor or a template disagrees with it.

### Changed

- **`flui-cli`**, **`flui-native`**: the Android `minSdk` the packager writes and the API level
  the backend assumes when the system will not report one are generated from that file instead of
  being literals.
