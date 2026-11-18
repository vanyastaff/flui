### Added

- `flui_protocol::native_recording::Budget` and `Overrun`: the count of a native recording's line, row and byte bounds that its writers and its reader share.
- `flui_native::journal::codec::Row::Gap` and `ReplayError::Gap`: a native recording holds a `gap` where its writer lost a row longer than a line may be; it reads back as a row and stops a replay.

### Changed

- A release build compiles none of the native input journal: `flui-native`'s recording codec, like its capture, is built only in a debug build with `native-capture` (or with `testing`), although `flui-app` turns the feature on in every build.
- `NativeRecording::write` refuses a header, or a row, longer than a recording line may be with `CodecError::LineTooLong` and writes nothing, and writes a recording within the bounds whole however close it comes to them.
- The in-process recorder holds a row longer than a line may be as a one-row `gap`, so a long input method commit no longer grows the recording without bound or makes it unwritable.

### Fixed

- `flui record --native` holds each stream's file to the bounds a native recording's reader takes: a header longer than a line opens no file, the first row past `MAX_ROWS` or `MAX_RECORDING_BYTES` is replaced by a `cut` row that ends the file, so a replay stops there instead of the reader refusing the whole file, and a row the file cannot hold leaves a `gap`. The index and the command's report count each row once, as written, lost or dropped at the cut, and name where a stream was cut.
