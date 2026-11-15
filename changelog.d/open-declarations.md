### Added

- `FileTypeDeclaration` and `OpenDeclarations::file_types`: the file types an application opens,
  next to its URL schemes and associated domains. `#[flui::main]` reads the manifest's
  `file-types` with them. A declared file type keeps the application one instance on Linux and
  Windows, as a declared URL scheme does.
