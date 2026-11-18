### Changed

- `flui-scheduler`'s modules are private: every public item is reached from the crate root
  (`flui_scheduler::UpdateScheduler`, not `flui_scheduler::scheduler::UpdateScheduler`), and the
  embedder entry points stay under `flui_scheduler::__runtime`.

### Removed

- `flui_scheduler::CallbackId` and the `id` module: a callback's id is
  `flui_foundation::FrameCallbackId`.
- The re-exports of `PresentationAddress`, `PresentationId` and `UiRuntimeId` from
  `flui-scheduler`; they are reached from `flui_foundation`.
