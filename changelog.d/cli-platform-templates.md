### Changed

- Build outputs (APK, `.app`, web page) go to `<target-dir>/flui-out/<project>/<platform>/`, where
  `<target-dir>` is the one cargo reports (following `CARGO_TARGET_DIR`, `build.target-dir` and an
  enclosing workspace), instead of `<project>/target/flui-out/<platform>/`. `cargo clean` removes
  them, and projects sharing a target-dir keep apart.

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
  every desktop target) removes the build's output in
  `<target-dir>/flui-out/<project>/<platform>/`, and each `--output` directory a build created (or found empty): the
  build leaves a `.flui-out` marker there naming the platform and project, and a directory that held
  anything before the first build into it, another platform's or project's output included, is
  never removed. For web it removed `platforms/web/pkg/`, which no build writes.
  `flui clean` without `--platform` removes every platform's `--output` directories too, before
  `cargo clean` removes the record of them; `--deep` still adds what the platform build tools write
  in `platforms/`. The record of those directories is only an index: losing or damaging it
  (`cargo clean`, deleting `target/` by hand) never fails a build or a clean, and the next build
  into a claimed directory records it again.
