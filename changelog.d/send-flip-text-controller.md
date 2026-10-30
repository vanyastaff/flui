### Changed

- `TextEditingController` keeps its text, selection and composition in owner-local state
  (`Rc<RefCell>`) instead of behind a mutex, so it is no longer `Send` or `Sync`. Listeners and
  `on_changed` run after the controller is released, so they may read it.
