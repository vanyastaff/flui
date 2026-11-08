### Changed

- Android drops now retain `ClipData` by offer key and read text, HTML, URI lists, and URI bytes lazily through `DragTransport::request`, with bounded UTF-8-aware reads and one-time URI permission release.
