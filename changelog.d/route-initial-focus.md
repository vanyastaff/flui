### Fixed

- **Same-depth rebuild order** (`flui-view`): the build drain now builds elements of equal depth in
  the order they were queued, so a freshly mounted subtree builds its siblings in child order. It
  used to order by depth alone, which let siblings build in an order that changed between mounts
  and runs; a pushed route's initial focus (the first control to attach) followed it, landing on
  different buttons, and drawing the focus overlay on them, from one mount to the next. A pushed
  route whose controls sit at the same depth now focuses the first of them in child order.
- **Rebuild order after a signal write or a provider change** (`flui-view`): readers of a signal
  are queued in the order they subscribed, and dependents of an `InheritedView` in the order they
  registered, instead of in a hash map's per-map order, so same-depth readers rebuild in the same
  order on every write.
