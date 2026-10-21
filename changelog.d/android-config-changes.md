### Fixed

- **`flui-cli` (Android)**: the generated `AndroidManifest.xml` declares every configuration
  change the host follows in place: `orientation`, `screenSize`, `smallestScreenSize`,
  `screenLayout`, `density`, `uiMode`, `fontScale`, `fontWeightAdjustment`, `locale`,
  `layoutDirection`, `keyboard`, `keyboardHidden` and `navigation`. A dark-mode, font-scale,
  bold-text, density or language change, or an attached keyboard, now arrives as
  `ConfigChanged` in the running app instead of recreating the Activity and restarting
  `android_main` with fresh state. A project scaffolded earlier keeps its old list in
  `platforms/android/app/src/main/AndroidManifest.xml`; copy the new `android:configChanges`
  value there.
- **`flui-native` (Android)**: a dark-mode change handled in place reaches
  `MediaQueryData::platform_brightness`. The host reads night mode from the Activity's
  `Configuration.uiMode`; the native configuration android-activity refreshes kept the night mode
  the Activity started with.
- **`flui-cli` (Android)**: the Gradle-less packager refuses an SDK whose newest `android.jar`
  is older than API 31 with a named error, since `fontWeightAdjustment` does not link against
  it.
