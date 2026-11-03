### Added

- `flui build ios` puts a `PrivacyInfo.xcprivacy` in the app bundle that declares the
  privacy-sensitive APIs FLUI calls: system boot time (`35F9.1`) and file timestamps (`C617.1`).
  An app's own `platforms/ios/PrivacyInfo.xcprivacy` keeps its tracking and collected-data
  entries, and its accessed APIs are joined with FLUI's.
- `flui build android` refuses a 64-bit library (`arm64-v8a`, `x86_64`), the hot-reload scene
  plugin included, whose loadable segments are aligned below 16 KB. A 16 KB-page device can't
  load such a library, and Google Play refuses updates that ship one from 2027-02-01.

### Changed

- Android APKs declare `targetSdk` 36 and compile against API 36 or later, the level Google Play
  requires of new apps and updates since 2026-08-31. The value is pinned in
  `platform-support.toml` (`android.target-sdk`), not taken from the newest installed platform.
  Gradle projects read it as `flui.targetSdk`, which `flui build` writes into
  `gradle.properties`.
- The APK packager that runs without Gradle aligns libraries for 16 KB pages
  (`zipalign -P 16`) and needs build-tools 35 or later.
