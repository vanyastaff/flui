### Added

- On Android what an intent brings reaches `AppHandler::opened`: the data of a `VIEW` intent (a
  link of the application's scheme, a verified app link, a document) and the streams of a share,
  from `onCreate` as a cold launch, so the first Router opens on the link, and from `onNewIntent`
  while the application runs. A recreated Activity and one started again from Recents do not open
  their intent twice.
- A `content:` document is opened for reading through the content resolver and reaches
  `Application::on_open_files` as an `OpenedFile` that holds the descriptor open while any clone
  of it lives (`/proc/self/fd/N`). `OpenedFile::name()` gives the name the provider shows, and a
  path's file name elsewhere; `OpenedFile::try_from_descriptor` makes one from a descriptor.

### Changed

- `FluiHost`'s protocol is 3: an APK must pack the `classes.dex` of the `flui-native` it links.
- `OpenedFile` compares by path and name; its `Debug` says whether it holds a descriptor.
