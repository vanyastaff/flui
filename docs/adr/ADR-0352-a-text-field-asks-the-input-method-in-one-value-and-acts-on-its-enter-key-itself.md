# ADR-0352: A text field asks the input method in one value, and acts on its Enter key itself

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0348](ADR-0348-android-text-input-loses-no-edit-and-restarts-the-keyboard-only-for-its-composition.md)
  §3 for the keys an editor action arrives as (Next and Previous were Tab and Shift+Tab)
- **Related:** [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (the text store whose
  status carries the value; §2's projection, whose preedit cursor this record reads as the
  target clause), [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §5 (the composing
  state is one value), [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (the
  recording mirrors the status field by field), [ADR-0325](ADR-0325-a-context-is-bound-as-ctx-and-a-description-builder-has-no-with.md)
  (an `…Options` type's builder has the bare name)

## Context and problem statement

`TextStoreStatus` said three things of a field: whether it is protected, whether it holds one
line, and its `TextInputPurpose` (text, email, URL, phone, a whole number, search). Every
platform's keyboard asks more, and the hosts guessed it:

| Fact | What the hosts did |
|------|--------------------|
| The Enter key's label | The Web wrote `enterkeyhint="search"` for a search and `"enter"` for many lines, Android `IME_ACTION_SEARCH` or `IME_ACTION_DONE`; no field could ask for Next, Go or Send. |
| Capitalization | The Web's hidden editor wrote `autocapitalize="off"` for every field; iOS answered nothing, so UIKit's default applied; Android never shifted. |
| Correction | iOS corrected prose and nothing else; Android's single-line field had no `TYPE_TEXT_FLAG_AUTO_CORRECT`, so the AOSP keyboard did not correct it; the Web's hidden editor wrote `autocorrect="off"` and `spellcheck="false"` for every field. |
| A number's sign and separator | `Number` was a whole number of digits; a temperature or a price could not ask for a minus sign or a decimal separator. |
| Autofill | A protected field was `current-password` on the Web's semantics field and `new-password` on its hidden editor, decided by the host; a sign-up form could not ask for a new password, nor a login form for a user name or a one-time code. |

Android delivered the Next and Previous editor actions as Tab and Shift+Tab (ADR-0348 §3), so a
field that could have asked for Next would have moved the focus on Android and submitted
everywhere else: iOS sends Return as `"\n"`, the Web sends `Enter`, a hardware keyboard sends
Enter.

Separately, `project_ime_event` kept only the end of a push preedit's cursor range. The range is
the input method's target clause (the part being converted); AppKit's `setMarkedText:selectedRange:`
and `text-input-v3`'s cursor already reached the store as a selection inside the composition, so
only the push path lost the clause.

The question: where does a field say what it asks of the input method, and who acts on the label
of its Enter key?

## Decision drivers

- One source per fact: a host passes a value on and decides nothing a field could ask for.
- An invalid state is unrepresentable: no combination that no platform has.
- Policy lives at the seam (AGENTS.md "Before each push" item 7): a behaviour one host holds
  either moves to where every host has it or is not a host's.
- A hardware keyboard and every platform's action key behave alike.
- The recording mirrors every field of the status, and an older recording still reads.

## Considered options

1. One `TextInputOptions` value in the status, seeded by the purpose; the Enter key's label only
   labels, every host delivers Enter, and the field acts on its label.
2. Flat booleans and enums beside `purpose` in the status, with the Android host mapping Next and
   Previous to Tab as before.
3. A field-level callback for the action key (Flutter's `onEditorAction`), delivered by each host
   as a new event.

## Decision

Chosen option: 1.

### §1. What a field asks is one value in its status

```rust
pub struct TextStoreStatus { pub protected: bool, pub single_line: bool, pub text_input: TextInputOptions }

#[non_exhaustive]
pub struct TextInputOptions {
    pub purpose: TextInputPurpose,          // Number(NumberFormat { signed, decimal })
    pub enter_action: EnterAction,          // Default, Done, Go, Next, Previous, Search, Send
    pub capitalization: Capitalization,     // None, Sentences, Words, Characters
    pub correction: Correction,             // Off, Suggestions, Autocorrect
    pub autofill: Option<Autofill>,         // Username, CurrentPassword, NewPassword, OneTimeCode
}
TextInputOptions::new(purpose).enter_action(EnterAction::Next).correction(Correction::Off)
```

- `TextInputOptions::new(purpose)` picks the defaults from the purpose: a search's Enter says
  Search, prose is autocorrected, an address, a telephone number or a number is not corrected.
  The purpose has no builder, so no default depends on the order the methods were called in.
- `Correction` is three states, not two booleans: a correction without suggestions exists on no
  platform.
- Protection wins over every option: a protected field is never shifted, corrected or learnt
  from, whatever it asks.
- Each host passes on what its platform has a word for, and `platform-parity.toml` states the
  rest: UIKit has no Previous key and no suggestions without corrections; an `EditContext` reads
  no capitalization or correction; `text-input-v3`, IBus and Fcitx5 have no Enter key label; TSF
  reads the purpose alone; Android's per-field autofill is not built.
- `EditableText::text_input(TextInputOptions)` sets it, and each field above it (`TextField`,
  `RawTextFormField`, the Material fields) forwards it whole.

### §2. The Enter key's label labels; the field acts on it

Every host delivers the on-screen keyboard's action key as the Enter key, whatever its label:
Android's `performEditorAction` included, for every action. `EditableText` answers Enter by its
own `enter_action`: after the submit callback, `Next` moves the focus to the next field and
`Previous` to the one before, unless the callback already moved it or let the field go. A
hardware Enter does the same.

### §3. A push preedit's cursor is the target clause

`project_ime_event` selects a preedit's cursor range inside the composition (anchor at its
start, the caret at its end), each end clamped forward to a `char` boundary and to the preedit's
end, as the pull paths do.

### Consequences

- Good, because a field asks for every keyboard behaviour the platforms share, and no host
  guesses one.
- Good, because Next and Previous behave alike on every host and from a hardware keyboard, and
  the submit callback runs for them on Android too.
- Good, because a field can mark the clause being converted on every input path.
- Bad, because a free-text field is now autocorrected on Android (the AOSP keyboard needs
  `TYPE_TEXT_FLAG_AUTO_CORRECT`) and on the Web's hidden editor, and iOS fields no longer shift
  sentences unless they ask: a visible change in existing applications, recorded in the
  changelog.
- Neutral, because the recording gains optional fields: an Enter action or a correction left
  out reads as the purpose's default, so an older recording reads as it was taken.

### Confirmation

- The options' defaults and builders: `flui-platform`'s types, with `exhaustive_enums` kept by
  `#[non_exhaustive]`.
- Each host's mapping, one table test beside it: `a_fields_options_pick_its_return_key_capitalization_correction_and_content`
  (iOS), `the_text_input_options_reach_the_editor_info` (Android),
  `the_capitalization_and_correction_a_field_asks_for_are_its_hints` (Wayland),
  `a_fields_capitalization_and_correction_reach_each_protocol` (IBus and Fcitx5),
  `the_status_picks_capitalization_and_correction` and `the_status_labels_the_enter_key` (the
  Web), `a_field_gets_the_scope_its_status_asks_for` (TSF); headless Chrome reads the attributes
  in `the_hidden_editor_states_each_purpose_and_enter_action`.
- §1 through the field: `flui-widgets`' `the_store_reports_the_text_input_the_field_was_built_with`
  and `a_push_platform_is_told_the_focused_fields_status`; the recording:
  `a_store_focus_keeps_every_field_of_its_status`.
- §2: `enter_moves_the_focus_as_the_fields_enter_key_says`.
- §3: `ime_events_project_onto_the_store_as_the_text_input_contract_says`.
- Runs owed on devices are rows of `docs/platform-parity.md` ("Input purpose and hints").

## Pros and cons of the options

### 1. One value, seeded by the purpose; the field acts on its label

- Good, because the field forwards one value through every layer, and the recording mirrors one
  value.
- Good, because the defaults follow the purpose without an "automatic" variant a host would have
  to resolve.
- Bad, because the purpose is given to the constructor, not set by a builder.

### 2. Flat fields beside the purpose, Android's Tab kept

- Good, because no type is added.
- Bad, because every field layer forwards four more builders.
- Bad, because Next submits on iOS, the Web and a hardware keyboard while it moves the focus on
  Android, and the submit callback runs on one and not the other.

### 3. An action callback each host delivers

- Good, because a field could tell the action key from a hardware Enter.
- Bad, because neither the Web nor iOS tells them apart (both send Enter), so the callback could
  not be delivered alike on every host.

## More information

- Flutter 3.50.0 `TextInputConfiguration` (`packages/flutter/lib/src/services/text_input.dart`):
  `inputType` with `numberWithOptions(signed, decimal)`, `inputAction`, `textCapitalization`,
  `autocorrect`, `enableSuggestions`, `autofillConfiguration`; its `EditableText` moves the focus
  for `next` and `previous` itself (`_finalizeEditing`).
- Android `EditorInfo` (`TYPE_TEXT_FLAG_CAP_*`, `TYPE_TEXT_FLAG_AUTO_CORRECT`,
  `TYPE_TEXT_FLAG_NO_SUGGESTIONS`, `TYPE_NUMBER_FLAG_SIGNED`, `TYPE_NUMBER_FLAG_DECIMAL`,
  `IME_ACTION_*`), UIKit `UITextInputTraits` (`returnKeyType`, `autocapitalizationType`,
  `textContentType`), HTML `enterkeyhint`, `autocapitalize`, `autocomplete`,
  `zwp_text_input_v3.content_hint`.
