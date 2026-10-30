### Added

- A text field's text publishes as AccessKit text runs, so a screen reader reads it by character,
  word and line and follows the caret and selection. The render object that lays out the text
  declares a `TextContent` of `TextRun`s (`SemanticsConfiguration::set_text_content`), checked
  against AccessKit's model by `TextRun::try_new` and `TextContent::try_new`; the pipeline gives
  each run an identity that lasts while its text does. `TextRunPosition` and `TextRunSelection`
  name places in the runs, and `TextContent` converts them to bytes of the value.
- `Semantics::on_set_text_selection` and `Semantics::on_replace_selected_text`.
- `SemanticsDiagnostic::TextRunsWithheld`: a field published without its runs and selection,
  because the presentation's synthetic identity space is exhausted.
- `SemanticsOwner::semantics_action_request_for` and `PipelineOwner::decode_semantics_action`
  decode a platform request against the published tree: a request on a run goes to its field,
  and a selection's run positions become bytes of the value.

### Changed

- `TextSelection`, the payload of `ActionCall::SetTextSelection`, is in UTF-8 bytes of the field's
  value. The free `semantics_action_request_for` refuses a `SetTextSelection` with
  `ActionDecodeError::InvalidTextPosition`: only the owner that published the runs can read it.
