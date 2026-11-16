### Added

- A native recording's `window` stream (debug builds with `native-capture`) holds the window's close requests with what its handler answered, its close with the cause, and every lock a pull-model input method (TSF, AppKit, UIKit, the Android and web mirrors, the Linux text inputs) held on the focused text store: each read and edit with the store's answer, and the document the lock found. Replayed on the headless host, a recorded composition leaves a fresh store with the same text, selection and composition, and a read or close decision that answers otherwise stops the replay with a typed divergence naming the row. A lock on a protected (password) field is recorded only as having happened: none of the field's text, ranges or lengths reach the recording.
- Agent protocol 0.13: the native recording rows `store_focus`, `store_lock`, `close_request` and `closed`, and the bounds a reader checks (`MAX_STORE_STEPS`, `MAX_STORE_TEXT_BYTES`, `MAX_LINE_BYTES`, `MAX_ROWS`, `MAX_RECORDING_BYTES`).

### Changed

- Agent protocol 0.13 is a breaking version: an app speaking it refuses a client of 0.12 or older, which cannot read the new native recording rows.
- Reading a native recording refuses a line longer than `MAX_LINE_BYTES` before parsing it, a recording of more than `MAX_ROWS` rows or `MAX_RECORDING_BYTES` bytes, and a text store focused twice, with typed errors naming the line.
