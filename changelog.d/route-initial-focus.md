### Fixed

- **Same-depth rebuild order** (`flui-view`): the build drain now builds elements of equal depth in
  the order they were queued, so a freshly mounted subtree builds its siblings in child order. It
  used to order by depth alone, which let siblings build in an order that changed between mounts
  and runs; a pushed route's initial focus (the first control to attach) followed it, landing on
  different buttons, and drawing the focus overlay on them, from one mount to the next. A pushed
  route now focuses its first control in child order.
