### Fixed

- An element whose lifecycle hook panics with a payload whose own `Drop` panics is still replaced by its error view: the payload is destroyed under containment instead of failing the build pass.
