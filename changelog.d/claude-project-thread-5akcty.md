### Added

- `flui_foundation::BuildPhase`, an owner-local view of "a `build` is running", with `LifecycleContext::build_phase`, `BuildOwner::build_phase`, `BuildInProgress::phase`, `Vsync::set_build_phase` and `Vsync::is_building`.
- `flui_foundation::MutatedDuringBuild`, the typed refusal of a controller mutation made from `build`, and `AnimationError::MutatedDuringBuild`.

### Changed

- Controllers refuse a mutation made while the widget tree builds (ADR-0086 §7): the state is unchanged and no listener is notified. `AnimationController::set_value`, `stop`, `reset`, `finish` and every run command return `Err(AnimationError::MutatedDuringBuild)`; `ScrollController::jump_to`/`animate_to`, `TabController::set_index`/`animate_to` and `CupertinoTabController::set_index` now return `Result<(), MutatedDuringBuild>`. Migration: mutate controllers from an event callback, a lifecycle hook or a post-frame callback; handle or discard the returned `Result` (`let _ = controller.set_index(i);`).
- `AnimationController::set_value` returns `Result<(), AnimationError>`, and a disposed controller answers `Err(AnimationError::Disposed)` instead of ignoring the call.
