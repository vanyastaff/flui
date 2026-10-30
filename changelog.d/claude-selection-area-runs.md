### Added

- **`flui-widgets`**: `SelectionArea` makes the static text beneath it selectable as one
  document. A screen reader reads each `Text` and `RichText` by character and word and moves
  the selection across paragraphs, and `on_selection_changed` reports the selected text in
  the order the document reads them. `SelectionContainer::disabled` opts a subtree out. Text outside an area is
  unchanged ([ADR-0214](/docs/adr/ADR-0214-selection-area-text-runs.md)). Pointer and keyboard
  selection, the highlight and touch handles follow.
- **`flui-semantics`**:
  - `SemanticsConfiguration::set_merge_boundary`: a node that keeps its own node under a
    merging ancestor without merging its descendants.
  - `DocumentSelection` and `DocumentPosition`: a selection across the texts beneath a
    document, published in their runs. A document receives it as
    `ActionCall::SetDocumentSelection`. `SemanticsOwner::document_texts` lists a document's texts in
    published order.
  - `Semantics::merge_boundary`, `document_selection` and `on_set_document_selection` expose
    these on the widget.
- **`flui-objects`**: `RenderParagraph::set_selectable` publishes the paragraph's runs under a
  selection area's anchor.
