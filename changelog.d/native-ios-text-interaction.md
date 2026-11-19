### Added

- On iOS, a focused text field gets UIKit's caret, selection highlight, handles, loupe and edit
  menu: a `UITextInteraction` is put over the field while it has the keyboard and taken off when
  it loses it. Its gestures begin only where a press lands on the field by the window's own hit
  test, or near a shown selection handle where nothing else takes it, so a control over the field
  or beside a handle keeps its tap. The field then draws none of these itself and its own taps,
  long presses and drags stay out of it; it keeps drawing its text and its composition's
  underline. What UIKit draws stays inside the part of the field the window shows, and is asked
  for again when the field itself moves, never on a caret move or while the keyboard composes.
- `TextStoreHost::selection_ui` and `SelectionUi` in `flui-platform`: whether the host's platform
  draws the served field's selection UI; `TextInputHandle::selection_ui` reads it for an attached
  field.
- `TextStore::edit_command_enabled`, `TextStore::perform_edit_command` and `EditCommand`: a
  platform's edit menu asks the focused field for Cut, Copy, Paste, Select (the word at the caret)
  and Select All, which `EditableText` performs with its own clipboard and selection rules. On iOS
  a command reaches only the field the menu was offered over, and Paste is offered while the
  pasteboard holds text.
- `TextStoreRead::press_target_at` and `PressTarget`: what a press at a point lands on (the
  field, another target, nothing), and `TextStoreRead::visible_bounds`: the part of the field the
  window shows, its ancestors' clips included. Both are required of every store session;
  `EditableText` answers them with the window's hit test, asking each target there whether it
  would take the press, and the pipeline's placement query.
- `PressProbe` and `InteractionDispatchHandle::declare_press_admission` in `flui-interaction`
  (`RenderObjectContext::declare_press_admission` and `press_probe` in `flui-view`): a pointer
  target says whether it would take the press a Down starts, and a platform's question is
  answered without delivering anything. A `Listener` declares it from its pointer-down handler
  and recognizers (`RecognizerSet::takes_press`, and `GestureRecognizer::takes_presses`, `false`
  for a drag, a scale, a swipe), and a text link takes the presses on its span.
- `PipelineOwner::presented_placement` and `PresentedPlacement` in `flui-rendering`: where one
  render node's local space is drawn in the window after the last frame and the part of the
  window its ancestors' clips leave it, by the paint walk's gates and composition, which the
  input-region walk and `presented_nodes` share.
- `TextStoreObserver::field_moved`: the field itself moved in the window, told apart from the
  layout change every caret move sends.
- `flui-testing`: `RecordingTextStoreHost::with_selection_ui`,
  `HeadlessWindow::with_text_store_host_drawing`, `harness::mount_with_selection_ui_on` and
  `lay_out_with_selection_ui_on`.

### Fixed

- On iOS, `selectionRectsForRange:` answers one rectangle per line of the range, with its writing
  direction and which rectangles hold the range's start and end, instead of none.
