### Added

- `flui_protocol::native_recording::Budget` and `Overrun`: the count of a native recording's line, row and byte bounds that its writers and its reader share.

### Changed

- A release build compiles none of the native input journal: `flui-native`'s recording codec, like its capture, is built only in a debug build with `native-capture` (or with `testing`), although `flui-app` turns the feature on in every build.
- `NativeRecording::write` refuses a row, or a header declaring the rows' devices, longer than a recording line may be with `CodecError::LineTooLong` instead of writing a file its reader refuses.

### Fixed

- `flui record --native` holds each stream's file to the bounds a native recording's reader takes: the first row past `MAX_ROWS` or `MAX_RECORDING_BYTES` is replaced by a `cut` row that ends the file, so a replay stops there instead of the reader refusing the whole file; a row longer than a line may be leaves a `gap`. The index and the command's report name where a stream was cut and how many rows were not written.
