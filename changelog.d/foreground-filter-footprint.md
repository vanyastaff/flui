### Fixed

- Foreground Blur, Compose and Morph filters retain bounded off-viewport input and preceding intermediate support, including nested clips and layers.
- Filter replay uses a signed attachment origin for drawing, scissors and masks; invalid radii or unrepresentable sigma report an error before GPU work.
