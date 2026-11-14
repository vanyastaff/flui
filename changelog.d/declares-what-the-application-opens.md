### Added

- `[package.metadata.flui.declares]` schema 3: `url-schemes`, `associated-domains` and
  `file-types` name what an application opens. `flui build` turns them into `FluiActivity`'s
  intent filters (a verified one for the domains' `https` links) and into the `CFBundleURLTypes`,
  `UTExportedTypeDeclarations` and `CFBundleDocumentTypes` of the iOS and macOS `Info.plist`.
