### Changed
- `Image::memory` and `Image::file` decode off the UI thread: the bytes decode on the compute lane, a file is read on the IO lane first, and the widget shows its placeholder until the decode answers. A rebuild with equal bytes or the same path decodes nothing again. `gapless_playback` now applies to them.
- `Image` is a stateful widget in every build, not only with `asset-images`; a custom provider's `cache_key` is honoured without that feature.

### Added
- `ImageProvider::decode_job` and `DecodeJob`: a provider hands `Image` work to run on the background lanes, a decode or a read followed by a decode.
- `ImageProviderError::Background`, for a decode step whose lane refused it, that panicked, or whose UI runtime ended.
