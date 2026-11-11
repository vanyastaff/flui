### Fixed

- A lazy list's item builder or `find_index_by_key` callback that panics with a payload whose own `Drop` panics no longer fails the frame: the payload is destroyed under containment, and the index shows its error view.
