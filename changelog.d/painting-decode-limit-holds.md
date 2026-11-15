### Fixed

- `Image::decode` measures an image at its own depth as well as RGBA8: a 16-bit PNG whose
  16-bit pixel buffer would pass `MAX_DECODED_IMAGE_BYTES` is refused with
  `ImageDecodeError::TooLarge`, naming that buffer's size, before it is allocated, where it was
  allocated past the limit.
