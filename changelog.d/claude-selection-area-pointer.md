### Added

- **`flui-semantics`**: `reading_order` orders boxes the way a reader takes them: line by line
  from the top, and along each line in a text direction. `SemanticsConfiguration::makes_document_selections`
  is public.

### Changed

- **`flui-widgets`**: a `SelectionArea` publishes its texts in reading order: explicit sort keys
  first, then where each text is laid out, in the ambient direction. A column laid out upwards, a
  stack or a right-to-left row reads and reports the way it looks, not in tree order.
