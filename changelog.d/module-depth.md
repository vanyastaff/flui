### Changed

- Rename the independent UI execution owner from `UiRealm` to `UiRuntime`, its
  generational identity from `RealmId` to `UiRuntimeId`, and the identity field
  in `PresentationAddress` to `ui_runtime_id`. The runtime's identity accessor
  is `id()`. Runtime module paths use
  `ui_runtime`; associated error variants and test-support names use runtime
  vocabulary. No compatibility aliases retain the former names.

### Fixed

- Tie frame resources and asynchronous renderer publication to the presentation
  that installed them. Native close and quit revoke publication before terminal
  lifecycle observers run.
- Preserve close-handler registrations made during another handler's retirement,
  and retire native owners outside app registry borrows while retaining the first
  failure across independent cleanup.
