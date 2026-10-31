### Added

- `Persisted<D>` is `Listenable`: its listeners are told when `status()` or `committed()` changes,
  on a later turn of the UI runtime's tasks and once for all the changes since they were last told,
  so a widget can rebuild with a document's save status, including a commit by another document of
  its name in the same runtime. A status that stands tells nobody and wakes no frame, and a listener
  may call any method of the document.
- `Persisted::revision` is the revision of the value in memory while it is the stored one: the
  file's after a load, a set value's once the storage confirms it, `None` otherwise, so it always
  names bytes on disk, also when another document of its name in the runtime wrote last.
  `Revision::NONE` is the revision of a document that was never stored.
- `Persisted::downgrade` gives a `WeakPersisted<D>`, which does not keep the document alive, for
  the document's own listeners to hold; `WeakPersisted::upgrade` gives the document back while a
  clone still holds it.
- With the `serde` feature (`flui-view/serde`, part of the facade's `serde`), `Revision` serializes
  as its number, so a document can record which revision of another it was taken against.

### Fixed

- A `Persisted` document whose frame hook panics while it queues work is left retryable, not stuck:
  an edit's save is watched again by the next edit, and an interrupted `start_over` is left failed
  with `StorageError::Cancelled`, so `retry` starts over. A save watch ended by such a panic no
  longer keeps later writes from being watched, and a delivery such a panic ends leaves the next
  one to come.
