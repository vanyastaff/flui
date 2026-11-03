### Fixed

- A Gradle Android project built by `flui build` takes its `applicationId` from `flui.toml`'s
  `[app] id` on every build, written into `gradle.properties` as `flui.applicationId`, instead of
  the package frozen when the project was scaffolded. Changing the identifier no longer leaves the
  APK under the old package while `flui run` starts and copies scene plugins into the new one. A
  build script that states another `applicationId` literal, or one the build cannot read, is
  refused with the line to write.
