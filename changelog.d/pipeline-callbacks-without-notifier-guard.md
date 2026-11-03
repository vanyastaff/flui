### Fixed

- A pipeline callback setter (`set_on_need_visual_update`, `set_on_semantics_owner_created`, `set_on_semantics_owner_disposed`) no longer waits for a visual-update wake running on another thread. A `RenderInvalidationHandle` mark ran the wake under the notifier's read lock, so a wake that waited for the owner thread, or re-entered a handle, deadlocked against the setter. Every callback now runs on an owned snapshot with no lock held; a wake already in flight finishes with the callback it took.
- `set_semantics_enabled(true)` queues the root's first semantics assembly before the semantics-owner-created callback runs, so a callback that panics no longer leaves semantics enabled with nothing ever assembled.
- `set_semantics_enabled(false)` delivers the semantics-owner-disposed callback even when the semantics owner's teardown panics (a dropped action handler, the announcement listener).
