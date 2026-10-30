### Changed

- Storage reaches widgets through the capability seam: `cx.capability::<StorageCapability>()` in a
  lifecycle hook hands out a `StorageHandle` (`flui::view::persist`). The framework provides it
  over the host's store when the run sets `AppConfig::with_storage_dir`, and an application
  replaces it with `Application::capability::<StorageCapability>(provider)`, an encrypted store
  for example. Without storage the acquisition is refused as `NotRegistered`, and as
  `NotOnThisPlatform` where the platform has no data root.

### Removed

- `LifecycleContext::storage`, `PresentationCapabilities::storage` and
  `RuntimeHostServices::with_storage`: acquire the `StorageCapability` instead.
