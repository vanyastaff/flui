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
- `flui clean --platform <android|ios|web>` removes the build's output in
  `target/flui-out/<platform>/`, and each `--out` directory a build created (or found empty): the
  build leaves a `.flui-out` marker there, and a directory that held anything before the first
  build into it is never removed. For web it removed `platforms/web/pkg/`, which no build writes.
  `flui clean --deep` cleans platforms before `cargo clean`, which removes the record of those
  directories.
