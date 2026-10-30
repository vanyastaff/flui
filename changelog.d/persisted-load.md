### Added

- `Persisted::load` reads the document through the UI runtime's storage: `Document::initial` when
  nothing is stored, otherwise the body under its `flui-document <name> <version> <revision>`
  header, decoded with an older format migrated by `Document::decode`. A missing or foreign header
  is `Corrupt`, a newer format leaves the document `ReadOnly`, a decode panic is contained as
  `Panicked`, a file over `Document::MAX_BYTES` is refused unread, and only the latest of several
  loads is applied. Dropping the future cancels the read.
