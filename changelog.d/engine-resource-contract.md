### Changed

- External GPU texture registration and replacement return typed errors and use explicit sampling, alpha and encoded-sRGB descriptors. Texture dimensions are derived from the allocation.
- Recorded external draws retain their original allocation through GPU completion; replacing a logical texture ID affects later recorded draws. Per-draw filtering and premultiplied/opaque alpha are honored.

### Fixed

- External textures under shader-mask layers resolve through the parent registry, including after allocation replacement or mask resize.

- Linear filtering of straight-alpha external textures premultiplies texels before interpolation, preventing dark halos and hidden colors from transparent texels.
