### Changed

- `flui run --agent` on an iOS Simulator names its `docs/gaps.md` entry when it refuses, as the Safari refusal does.

### Fixed

- `flui build android` and `flui run` on an Android device pass `zipalign` its alignment operand: without it every APK build failed at the align step with `zipalign`'s usage text.
