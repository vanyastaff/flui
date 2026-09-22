# Native iOS application

FLUI builds the Rust executable and creates its native UIKit application bundle.
No Flutter Runner or Xcode project is required.

- `flui build ios`: stage an unsigned device `.app` (signing is a separate step).
- `flui build ios --simulator <UDID>`: build for an available iOS simulator.
- `flui run --device <UDID>`: build, install and launch on that exact simulator.
- `flui build ios --lib --universal`: deliver a static-library XCFramework.

Use `xcrun simctl list devices available` to select a simulator UDID. Xcode and
the corresponding Rust iOS target must be installed. Application identity and
version come from `flui.toml`, falling back to Cargo package metadata.
