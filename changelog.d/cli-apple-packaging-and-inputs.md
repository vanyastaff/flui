### Added

- **`flui-cli`**: `[app] display_name` in `flui.toml`, the name the application is shown by
  (free text, up to 64 characters): the Apple bundles' `CFBundleDisplayName`, the Linux desktop
  entry's `Name`, the Android launcher label and the web page's title. Without it they show
  `[app] name`.
- **`flui-cli`**: `[app] build` in `flui.toml`, the build number both stores compare: Android's
  `versionCode` and Apple's `CFBundleVersion`. A release build of a pre-release version
  (`1.4.0-beta.2`) needs it, since every pre-release and the release would otherwise share one
  build number, which App Store Connect and Google Play refuse the second time
  ([ADR-0361](/docs/adr/ADR-0361-a-package-owns-only-the-file-types-its-application-defines-and-states-one-build-number.md)).

### Changed

- **`flui-cli`**: breaking: `flui.toml`'s `[app] name` is checked when the file loads, as
  `flui create` checks a project name (letters, digits, `-` and `_`, not starting with a digit,
  not a Rust keyword). A project whose name holds a space or punctuation (`name = "My Notes"`)
  renames it to a crate-like name (`name = "my-notes"`) and moves the shown name to
  `display_name = "My Notes"`. A `flui.toml` larger than 1 MiB is refused before it is read.
- **`flui-cli`**: a version whose derived Android `versionCode` would lose its order or leave Google
  Play's range (a minor or patch above 99, `0.0.0`) is refused when `flui.toml` loads, for every
  target, instead of only when an APK is packaged.
- **`flui-cli`**: in an Apple bundle, a file type the system declares (`png`, `jpg`, `pdf`, `docx`,
  …) is opened by the system's own type as an alternate viewer instead of being exported and owned
  under the application's identifier; a type only a linked crate declares is imported and opened
  as an alternate viewer, with a warning; only the application crate's own types are exported and
  owned
  ([ADR-0361](/docs/adr/ADR-0361-a-package-owns-only-the-file-types-its-application-defines-and-states-one-build-number.md)).
- **`flui-cli`**: the development server of `flui run --device browser:…` copies each file of the
  build from disk as it writes it, ends a response a client stops reading after 60 seconds,
  answers 500 for a page larger than 4 MiB instead of reading it whole, and answers 404 for a FIFO,
  socket or device in the served directory instead of opening it.

### Fixed

- **`flui-cli`**: the macOS `.app`'s `Info.plist` states the application's version
  (`CFBundleShortVersionString` as `major.minor.patch`, `CFBundleVersion` as the build number,
  `FLUIVersion` as written) instead of `1` and `1.0`, as the iOS bundle does; without `flui.toml`
  it states the built package's version.
- **`flui-cli`**: `flui build ios --lib` delivers the XCFramework of the libraries it built even
  when `platforms/ios/flui.xcodeproj` exists, instead of running `xcodebuild` on that project and
  returning its `flui.app`.
- **`flui-cli`**: a crate that gives one file extension two MIME types for two targets of one build
  is refused, as two crates doing so already were.
