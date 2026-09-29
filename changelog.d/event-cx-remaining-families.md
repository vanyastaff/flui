### Added

- `DraggableCanceledDetails`, the one value `Draggable::on_draggable_canceled` receives: the release velocity and the displacement since the drag started.

### Changed

- `Draggable`'s `on_drag_started`, `on_drag_update`, `on_drag_end`, `on_draggable_canceled` and `on_drag_completed` take `&mut EventCx` first, accept signal-write results and no longer require `Send + Sync`. A drag cancelled by unmounting the draggable reports through a live context, and the feedback layer is removed before that cancel runs user code.
- `PageView::on_page_changed` takes `&mut EventCx` and the page, no longer requires `Send + Sync`, and runs after the frame that next rebuilds the page view: every recorded page in order, through the callback current at delivery, never inside a build.
- `Action::invoke` takes the key event's `&mut EventCx` before the intent; `CallbackAction::new` and `CallbackShortcuts::binding` take `Fn(&mut EventCx, ..) -> R` closures that may return a signal-write result.

### Removed

- `Actions::maybe_invoke`: an action runs from key dispatch, with that dispatch's event context; `build` has none to give it.
