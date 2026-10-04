### Changed

- Image decoding borrows embedded encoded bytes and consumes the decoder's RGBA buffer instead of copying these buffers.
