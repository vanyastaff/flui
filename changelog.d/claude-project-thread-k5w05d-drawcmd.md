### Changed

- A `DisplayList` records each operation against an interned transform: operations under one transform share one table entry, and a command is 224 bytes instead of 344. Reading a list yields `DrawCommand<'_>`, a borrowed `{ transform: &Matrix4, op: &DrawOp }`, through `DisplayList::iter`, `DisplayList::get` and `&DisplayList: IntoIterator`.
- `flui_testing`'s recorded draw operations are `DrawnOp { transform, op }` values.

### Added

- `flui_painting::DrawCommands`, the iterator over a display list's commands.

### Removed

- `DisplayList::commands`, `AsRef<[DrawCommand]>` and `Index<usize>` for `DisplayList`: a list holds no slice of commands. Read it with `iter` or `get`.
- `DrawCommand::untransformed`.
