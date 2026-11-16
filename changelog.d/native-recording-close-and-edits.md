### Added

- A native recording's `window` stream (debug builds with `native-capture`) holds the window's close requests with what its handler answered, its close with the cause, and every lock a pull-model input method (TSF, AppKit, UIKit, the Android and web mirrors, the Linux text inputs) held on the focused text store: each read and edit with the store's answer, and the document the lock found. Replayed on the headless host, a recorded composition leaves a fresh store with the same text, selection and composition, and a read or close decision that answers otherwise stops the replay with a typed divergence naming the row. A protected (password) field gives one row while it stays protected: none of its text, ranges, lengths or the number of keys typed reach the recording.
- Agent protocol 0.14: the native recording rows `store_focus`, `store_lock`, `close_request`, `closed` and `cut`, a header that says a recording joined its stream late, and the bounds a reader checks (`MAX_STORE_STEPS`, `MAX_STORE_TEXT_BYTES`, `MAX_LINE_BYTES`, `MAX_ROWS`, `MAX_RECORDING_BYTES`).

### Changed

- Agent protocol 0.14 is a breaking version: an app speaking it refuses a client of 0.13 or older, which cannot read the new native recording rows.
- A native recording that reaches the byte or row bound ends with a `cut` row, in what `NativeRecording::write` writes and in what the in-process `Recorder` holds, which keeps nothing after the cut, instead of a file its reader refuses. `flui record --native` writes its lines as they come and does not apply the bounds yet (`docs/gaps.md`).
- Reading a native recording refuses a line longer than `MAX_LINE_BYTES` before parsing it, a recording of more than `MAX_ROWS` rows or `MAX_RECORDING_BYTES` bytes, a row after a `cut`, a text store focused twice, a lock on a store the recording never focused (unless it joined late), and close rows in an order no window writes, each with a typed error naming the line.
