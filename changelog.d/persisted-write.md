### Added

- `Persisted::set` writes the value through the host's flush registry under the next revision: a
  data document over the version it was loaded from, a machine-local one in place. The status is
  `Saving` until the storage confirms the write, then `Clean`, and `committed` reaches the
  revision; a refused write is `Failed` with the storage's error until `retry` writes the latest
  value again. An encode panic is contained as `Failed(Panicked { during: Encode })` and publishes
  nothing; the first failure is kept until a write succeeds.
- While a write is in flight the document holds its presentation's close for the user and the
  program (a session end goes through); after a failed write it requires a decision.
- `PersistError::NotLoaded`: `set` before a load succeeded.
