### Added

- `[package.metadata.flui.declares]` takes `apple.privacy.accessed-apis` (a required-reason
  `category` and its `reasons`), at the top or under `features.<feature>`. The iOS bundle's
  `PrivacyInfo.xcprivacy` lists what the crates the target links declare, joined with the
  application's own `platforms/ios/PrivacyInfo.xcprivacy`.

### Changed

- The privacy manifest's accessed APIs come from `flui-native`'s and `flui-assets`' declarations
  instead of a list in `flui-cli`; an unknown category or a malformed reason code fails the build
  naming the crate.
