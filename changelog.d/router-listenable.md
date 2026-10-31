### Added

- `RouterHandle<R>` is `Listenable`: its listeners are told when the router's stack changes, by an
  edit through any handle or a pop the navigator makes itself, on a later turn of the UI runtime's
  tasks and before any transition ends, once for all the edits since they were last told. A popup
  is not a change and neither is a stack that stands; a listener reads the stack with `stack()`.
  A committed edit is told even when user code it runs panics, and an unmounted router drops its
  listeners.
