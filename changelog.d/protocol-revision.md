### Changed

- `flui_protocol::TreeRevision` keeps its number private: a tree's owner advances it with `next`,
  which panics when the revision space is spent instead of stopping, so two different trees
  never share a revision. `TreeRevision::ZERO` is the revision of a tree that has published
  nothing, and `get` reads the number.
- `flui_protocol::LineReader` takes each line in time linear in the line, however many lines
  one read holds.
