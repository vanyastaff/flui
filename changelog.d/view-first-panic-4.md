### Fixed

- A lazy list's item builder or `find_index_by_key` callback that panics with a payload whose own `Drop` panics no longer fails the frame. The payload is destroyed under containment: the lookup declines the move, and the builder's index shows its error view.
