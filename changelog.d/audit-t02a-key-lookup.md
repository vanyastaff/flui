### Changed

- **Breaking:** `GlobalKey::current_element` returns `flui_view::KeyLookup` instead of
  `Option<ElementId>`: `Mounted(id)`, `NotMounted` (a registry was consulted and holds no element
  for the key), `NoActiveRuntime` (the call ran outside a UI runtime scope, so nothing was
  consulted) or `RegistryBusy` (called from inside the frame of the presentation that hosts the key).
  `GlobalKey::with_current_state` still returns `Option` and folds all three misses into `None`.
