### Added

- `flui doctor` checks a project's manifest declarations: inside a FLUI project, `project.declarations`
  fails on a malformed `[package.metadata.flui.declares]` and `project.android-manifest` on a
  hand-written Android manifest, listing what it lacks. `--manifest PATH`, repeatable, checks an
  `AndroidManifest.xml` or XML `PrivacyInfo.xcprivacy` FLUI does not write against what the linked
  crates declare (`project.manifest`), with the lines to add; an Android file that is not Gradle's
  merged manifest carries a `note` saying so.
- `doctor.check` events carry a `note` field (`null` when there is none).
