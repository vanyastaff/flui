### Added

- **`flui-painting`**: `Image::decode` (feature `decode`) is the one bounded decoder for
  untrusted image bytes. It reads the header first and refuses an image whose RGBA8 buffer
  would pass `MAX_DECODED_IMAGE_BYTES` with `ImageDecodeError::TooLarge { requested, limit }`
  before allocating; any other failure is `ImageDecodeError::Malformed`.
  `MAX_ENCODED_IMAGE_BYTES` bounds the encoded bytes a loader reads.

### Removed

- **`flui-assets`**: `From<image::ImageError> for AssetError`. Image decoding goes through
  `flui_painting::paint::Image::decode`, and its errors become `AssetError::LoadFailed`.

### Fixed

- **`flui-widgets`**: `MemoryImage` and `FileImage` decoded with no limit, so a small file
  whose header claimed 60000×60000 pixels allocated about 14 GB, and `FileImage` read a file
  of any size into memory. Both now refuse with `ImageProviderError::DecodeFailed` or
  `ImageProviderError::ReadFailed` before allocating.
