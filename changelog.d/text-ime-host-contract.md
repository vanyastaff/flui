### Added

- **`flui_platform_api::text_store::TextStoreHost`**: the owner-thread side of a pull-model
  window's text input, with `CompositionEnd { Committed, Abandoned, Deferred }` and
  `TextStoreHostError { NotFocused, Unavailable }` (ADR-0135). `complete_composition(&store)`
  ends the composition in the store it names, if the host serves that store.
  `flui_platform::OwnerPlatform::text_store_host` reads a window's host; no production backend
  offers one yet.
- **`flui_platform_api::text_store::commit_composition_in_place`**: clear a store's composing
  range, keeping its text, when the platform cannot end the composition.
- **`TextInputOwner::complete_composition`** and **`TextInputHandle::complete_composition`**:
  commit the active field's composition, keeping its text, through the host or in place.
- **`flui_platform::HeadlessPlatform::with_text_store_host`**: headless windows that offer a
  text-store host, for tests of the pull path.
- **`flui_testing::RecordingTextStoreHost`**, `HeadlessWindow::with_text_store_host`,
  `HeadlessHost::text_store_host`, `widgets::harness::Harness::store_host_calls` and, under
  `test-support`, `flui_runtime::UiRealm::text_input_handle`.

### Changed

- **`TextInputOwner::new`** takes a `TextInputBackend` (`Push(Arc<dyn PlatformTextInput>)`,
  `Pull(Rc<dyn TextStoreHost>)` or `Unsupported`) instead of an
  `Option<Arc<dyn PlatformTextInput>>`: `new(Some(platform))` becomes
  `new(TextInputBackend::Push(platform))`, `new(None)` becomes
  `new(TextInputBackend::Unsupported)`.
- **`PresentationWindow`** carries the window's text-store host
  (`PresentationWindow::with_text_store_host`) and is no longer `Send`.
- **`widgets::harness::mount_with_ime`** mounts a pull-model window; a test that reads
  `cursor_area_calls` or `ime_allowed_calls` uses the new `mount_with_push_ime`.

### Removed

- **`TextInputOwner::active_store`** and the test-support `UiRealm::active_text_store`: a test
  reads the focused store from the recording host (`Harness::active_text_store`).
