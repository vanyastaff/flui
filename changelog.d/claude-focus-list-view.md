### Added

- **`flui-widgets`**: `ListView::focusable` makes a list one Tab stop: Tab enters the remembered
  or first visible row, the arrow keys, Page Up/Down, Home and End move between rows, and the
  focused row stays built and announced while the pointer scrolls it away. `semantics_label` and
  `empty_label` name the list; an empty focusable list is still a Tab stop announced by its empty
  label ([ADR-0265](/docs/adr/ADR-0265-pinned-lazy-children-and-the-focusable-list.md) §3).
- **`flui-view`**, **`flui-widgets`**: `semantics_label` on `SliverList`, `SliverFixedExtentList`,
  `SliverGrid` and `GridView` names the collection's own list node.
