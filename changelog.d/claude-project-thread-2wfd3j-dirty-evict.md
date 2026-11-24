### Fixed

- Removing render objects evicts them from the layout, paint and failure sets by lookup, so a
  removal costs the number of removed nodes instead of the number of dirty nodes.
