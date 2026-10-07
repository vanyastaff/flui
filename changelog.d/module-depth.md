### Changed

- Rename the independent UI execution owner from `UiRealm` to `UiRuntime`, its
  generational identity from `RealmId` to `UiRuntimeId`, and the identity field
  in `PresentationAddress` to `ui_runtime_id`. The runtime's identity accessor
  is `id()`. Runtime module paths use
  `ui_runtime`; associated error variants and test-support names use runtime
  vocabulary. No compatibility aliases retain the former names.
