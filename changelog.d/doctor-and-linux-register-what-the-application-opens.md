### Added

- `flui build linux` writes a desktop entry (`share/applications/<id>.desktop`, `Exec=<executable> %U`)
  whose `MimeType` names the application's URL schemes and file types, and a shared-mime-info
  package giving each file type its extension, when the application declares them.
- `flui doctor --manifest` checks what the application opens: a hand-written Android manifest's
  intent filters (schemes, verified `https` hosts, MIME types) and an XML `Info.plist`'s URL schemes
  and document types. `project.declarations` counts what the application opens.
