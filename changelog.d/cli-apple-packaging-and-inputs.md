### Changed

- **`flui-cli`**: `flui.toml`'s `[app] name` is checked when the file loads, as `flui create`
  checks a project name (letters, digits, `-` and `_`, not starting with a digit, not a Rust
  keyword), and a `flui.toml` larger than 1 MiB is refused before it is read; a name with a quote,
  a `$`, a space or a slash no longer reaches the generated Gradle, manifest or web files.
- **`flui-cli`**: a file type the system declares (`png`, `jpg`, `pdf`, `json`, …) is opened in an
  Apple bundle by the system's own type, as an alternate viewer, instead of being exported and
  owned under the application's identifier; the application's own types are still exported and
  owned ([ADR-0361](/docs/adr/ADR-0361-an-apple-bundle-owns-only-the-file-types-the-system-does-not-declare.md)).
- **`flui-cli`**: the development server of `flui run --device browser:…` copies each file of the
  build from disk as it writes it, ends a response a client stops reading after 60 seconds, and
  answers 500 for a page larger than 4 MiB instead of reading it whole.

### Fixed

- **`flui-cli`**: the macOS `.app`'s `Info.plist` states `[app] version` (`CFBundleVersion` and
  `CFBundleShortVersionString` as `major.minor.patch`, `FLUIVersion` as written) instead of `1`
  and `1.0`, so an update of the application is no longer refused as the same version; without
  `flui.toml` it states the built package's version, as the iOS bundle does.
- **`flui-cli`**: `flui build ios --lib` delivers the XCFramework of the libraries it built even
  when `platforms/ios/flui.xcodeproj` exists, instead of running `xcodebuild` on that project and
  returning its `flui.app`.
- **`flui-cli`**: a crate that gives one file extension two MIME types for two targets of one build
  is refused, as two crates doing so already were.
