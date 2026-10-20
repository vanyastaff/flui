### Added

- **`ViewState::apply_view`**: a hook that receives the current view and a `BuildContext` right
  before a build that follows a first mount, a new view configuration or a dependency change, for
  state that derives something from its view and so cannot do it in `build(&self)`.
  Panics in it are contained and reported as `LifecycleHook::ApplyView`.

### Fixed

- **`Scrollable` and the widgets built on it** (`AnchoredDrag`, `RefreshIndicator`, `InteractiveViewer`)
  no longer run user code during `build`: the scroll owner, the command queued before the scrollable
  mounted, and the slots the gesture callbacks read are set up in `apply_view`. A listener of that
  command's run that wrote a signal was refused with `WrittenDuringBuild`.
