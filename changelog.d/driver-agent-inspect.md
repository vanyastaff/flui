### Added

- **Reading the trees through the development agent** (`flui-view`, `flui-runtime`,
  `flui-foundation`; ADR-0238): `AgentWindow::inspect` reads a window's element (`Widgets`) or
  render (`Render`) tree, or a subtree under a `root`, as the typed snapshot the tree describes
  itself with, limited by depth and node count (at most 4096 nodes). A `root` is a tree-local
  handle or a `NodeRef`; a reference to another window answers `unknown_handle`, a root that left
  its tree `gone`. The layer tree is read from the next scene the window shows (a window holds no copy
  of a painted scene): the read waits for that frame, and a window already holding 16 such reads
  answers `busy`.
- `NodeRef` (`flui-foundation::inspect`): a node of a given window as one text (`p3/v3.2`), with
  `Display` and `FromStr`; `Link` parses from its handle text.
