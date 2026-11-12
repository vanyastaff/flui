### Changed

- `ElementTree::activate` raises a subtree's activation panic through `FirstPanic::resume`: while the thread is already unwinding, the panic is discarded instead of aborting the process.
