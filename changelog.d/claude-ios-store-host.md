### Changed

- iOS text input: the keyboard's responder edits the focused field through the shared text-store host, the one macOS serves it through. Edits are applied through the field's store, which also answers selection, marked text, ranges and their rectangles. The keyboard learns about the application's own edits, and about the field's status (secure entry, keyboard type, autocorrection) from the field's store. Return and Backspace still reach the field as key presses, so a single-line field submits on Return.
- Text stores publish application edits through one observer transaction: the input method's before callbacks read the previous document and its after callbacks read the new document. `TextStoreChange::Composition` and `TextStoreObserver::composition_changed` report composition metadata separately, with a selection refresh. The text-store conformance kit advances to version 4; its required `TextStoreFixture::application_editor` supplies an application edit without a platform session or frame pump.

### Fixed

- iOS character ranges keep supplementary Unicode scalars whole in plain and protected fields. Point-derived ranges select the containing character in one store snapshot; outside points have no range, and closest-position queries retain nearest-caret behavior. A failed store read preserves accepted focus changes and the first failure. The text-store contract specifies offset validation before protected-text refusal.
- Publication drains contain each eligible queued grant and settlement separately, so one failure cannot abandon later accepted work. Reentrant application changes are published before a later platform write consumes them; closed-gate work stays deferred for its owner anchor.
- Apple text-input callbacks finish accepted focus and composition work after a contained failure. Retirement detaches the field even when its keyboard notification panics, and replacing a context releases its secure-input hold.
- Browser text input clears stale composing-key state when the application commits marked text without changing its bytes, on both EditContext and textarea surfaces.
