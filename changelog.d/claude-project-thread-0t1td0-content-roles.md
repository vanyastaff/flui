### Changed

- **A control over content is one named node** (`flui-semantics`, `flui-rendering`): a button,
  link, checkable control, menu item, tab, option or tree item takes the content beneath it
  that carries only a name (image, heading, text-level roles), keeps its own role and gains
  their names (ADR-0332). A button over a labelled image is announced "Add, button" and found
  by that name, where it was a nameless button with an image beneath it. A meter or a progress
  bar inside a control stays a node. Content that must stay a stop of its own, or be found by
  its `identifier`, is wrapped in `Semantics::container(true)`: an identifier on absorbed
  content is not published.

### Removed

- `SemanticsConfiguration::is_compatible_with`: node formation asks
  `SemanticsConfiguration::can_absorb`, `can_join` and `can_stay_beside`, with the scope
  `SemanticsConfiguration::names_a_control` carries down.
