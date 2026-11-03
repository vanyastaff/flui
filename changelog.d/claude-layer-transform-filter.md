### Fixed

- `ImageFilterLayer` with `ImageFilter::Transform`, alone or inside `ImageFilter::Compose`, draws its content through the transform; before, the frame failed with `UnsupportedImageFilter`. Damage under such a layer follows where its content moves.
