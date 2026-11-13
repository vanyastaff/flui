### Changed

- **A control over content is one named node** (`flui-semantics`, `flui-rendering`): a button,
  link, checkable control, menu item, tab, option or tree item takes the content roles beneath
  it (image, heading, text-level roles, meter, progress bar), keeps its own role and gains
  their names (ADR-0332). A button over a labelled image is announced "Add, button" and found
  by that name, where it was a nameless button with an image beneath it. Content that must stay
  a stop of its own is wrapped in `Semantics::container(true)`.

### Removed

- `SemanticsConfiguration::is_compatible_with`: node formation asks
  `SemanticsConfiguration::can_absorb` and `can_absorb_both` from the node's side.
