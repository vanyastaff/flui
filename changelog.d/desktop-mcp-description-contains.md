### Added

- **`find` and `wait_for` match `description_contains`** (`flui-desktop-mcp`): a case-insensitive
  substring of a node's description, so a paragraph, caption or other role that takes no name is
  found by its text. A node whose description was cut or could not be read matches no property
  query, as with its name.
