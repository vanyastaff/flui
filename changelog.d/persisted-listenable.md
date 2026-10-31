### Added

- `Persisted<D>` is `Listenable`: its listeners are told when `status()` or `committed()` changes,
  on a later turn of the UI runtime's tasks and once for all the changes since they were last told,
  so a widget can rebuild with a document's save status. A status that stands tells nobody and wakes
  no frame, and a listener may call any method of the document.
