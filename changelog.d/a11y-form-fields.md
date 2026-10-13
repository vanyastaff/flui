### Added

- **Form-control semantics** (`flui-semantics`, `flui-widgets`): the required and invalid states,
  `SemanticsConfiguration::set_required`/`set_invalid`/`set_placeholder` and the
  matching `Semantics::required`/`invalid`/`placeholder` reach AccessKit's `required`, `invalid`
  and `placeholder`. `EditableText::semantic_label`/`placeholder`/`invalid` and
  `RawTextField::invalid` set them on a field's node; `EditableText::required` and
  `TextField::required` mark a field the form needs filled in.

### Fixed

- **Text fields had no accessible name** (`flui-material`): a `TextField`'s node now carries its
  decoration's label as its name, its hint as its placeholder and its error as an invalid state;
  `InputDecorator` no longer announces the label and hint a second time as loose text when it
  decorates a `TextField` (a standalone decorator keeps them readable), and its
  error line is a polite live region. `RawTextFormField` marks a field with a validation error
  invalid.
