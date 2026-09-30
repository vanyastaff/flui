### Changed

- Build outputs (APK, `.app`, web page) go to `<target-dir>/flui-out/<project>/<platform>/`, where
  `<target-dir>` is the one cargo reports (following `CARGO_TARGET_DIR`, `build.target-dir` and an
  enclosing workspace) and `<project>` is the package in the project directory, instead of
  `<project>/target/flui-out/<platform>/`. `cargo clean` removes them, and members of one
  workspace keep apart; same-named projects sharing a target-dir share them, as their same-named
  binaries do.

### Fixed

- `flui create` and `flui platform add android` no longer scaffold a Gradle build script that moves
  the build directory to `platforms/build/`: the APK lands in `app/build/outputs/apk/<profile>/`,
  where `flui build android` reads it. The Android app no longer applies the Kotlin plugin, so the
  APK carries no `kotlin-stdlib`.
- The scaffolded `platforms/android/.gitignore` tracks the Gradle wrapper (`gradlew` decides
  whether `flui build android` uses Gradle) and ignores Gradle's root `build/` and the copied
  `app/src/main/jniLibs/`, which `flui clean --platform android` now also removes.
- The iOS, Windows, Linux and macOS `.gitignore` files list only what FLUI's builds write instead
  of Flutter's entries; the project `.gitignore` drops `flui.lock` and `/build`, which nothing
  writes.
- The web `index.html` and `manifest.json` no longer reference a favicon and icons the scaffold
  never writes.
- `flui clean --platform <android|ios|web|desktop>` (`desktop` is new, one output directory for
  every desktop target) removes the build's output in `<target-dir>/flui-out/<project>/<platform>/`;
  for web it removed `platforms/web/pkg/`, which no build writes. `flui clean` without
  `--platform` removes every platform's default output before `cargo clean`, and `--deep` adds
  what the platform build tools write in `platforms/`. An `--output` directory is the user's to
  clean. A directory `flui clean` cannot remove or inspect (an executable still running on
  Windows) no longer stops it: the rest, the other platforms and `cargo clean` still run, and the
  first failure is the command's error.
