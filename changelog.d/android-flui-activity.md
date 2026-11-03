### Changed

- Android apps run under `dev.flui.platform.FluiActivity`, a `NativeActivity` subclass the generated
  manifest names with `hasCode="true"`, `launchMode="singleTop"` and
  `enableOnBackInvokedCallback="true"`. FLUI's Java classes ship in the APK instead of being loaded
  from the native library: `flui build` packs the dex the linked `flui-native` declares, and a
  Gradle project compiles the declared sources through `flui.javaSources` (a build script that does
  not read it is refused). `flui run` starts `FluiActivity`.
- `[package.metadata.flui.declares]` schema 2 adds `android.dex` and `android.java`.

### Added

- `PlatformError::HostProtocol`: `android_platform` refuses to start over Java classes from another
  FLUI version, naming both protocols, and over an APK without them or with another Activity.
