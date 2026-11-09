### Added

- **`FrameFailureHandlerFactory`** (`flui-runtime`, re-exported by `flui-app`): a `Send + Sync`
  factory that builds one `FrameFailureHandler` for each runtime it is installed on.

### Changed

- **`FrameFailureHandler` is owner-local** (`flui-runtime`): `FrameFailureHandler::new` no longer
  needs `Send + Sync`, so a handler may capture an `Rc`; it is called only on its runtime's owner
  thread.
- **`AppConfig::with_frame_failure_handler` takes a `FrameFailureHandlerFactory`** (`flui-app`),
  and `AppConfig::frame_failure_handler` holds one. Each window's runtime now gets a fresh handler
  from the factory instead of every window sharing one handler, so state a handler captures by
  value (a counter, an escalation policy) is per window. To see every window's failures
  together, share that state from the factory, for example through an `Arc<Mutex<…>>` each
  handler clones.
