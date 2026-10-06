### Added

- **`TextEditingController::committed_text`** (`flui-widgets`): the text with the IME
  composition replaced by what it stands for (nothing for a new preedit, the original words for a
  reconversion), the text a field's owner works with
  ([ADR-0090](/docs/adr/ADR-0090-ime-pull-text-store-contract.md) amendment).
- **`CommitGate::defer_failure` and `take_failure`** (`flui-platform-api`): a panic caught while
  a text store settled a grant waits at its presentation's gate for the owner to report; the
  first is kept, later ones are retained.
- **`InMemoryTextStore::set_owner_listener`** (`flui-platform-api`): run code in the reference
  store's owner notification, as a field's `on_changed` runs.
- **Text-store kit version 2** (`flui_testing::text_store_kit::KIT_VERSION` is 2):
  `composition_over_a_selection_replaces_the_selection`,
  `composition_only_sessions_do_not_notify_the_owner`,
  `owner_notification_runs_after_release` and
  `reconverting_committed_text_notifies_only_on_commit`, with
  `TextStoreFixture::set_owner_hook` (a provided method; a fixture keeping its default fails
  the owner case). A suite pinned to version 1 is unchanged.
- **`text_store::CompositionLedger` and `text_store::committed_text`** (`flui-platform-api`):
  how a store keeps what its composition stands for through a session's edits.

### Changed

- **`LockArbiter::request` and `run_deferred`** take a second function, `settle`, called after
  each grant releases its lock and before the next queued grant: a store notifies its owner
  there. A panic in it goes to the store's `CommitGate` and the queue keeps running.
- **`EditableText::on_changed`** receives the committed text and is called only when it
  changes, after the input method's lock is released: a session that only composes or cancels a
  composition no longer calls it, and a lock `on_changed` requests is granted. Controller
  listeners also run after the lock is released.
- **A text form field** validates and saves the committed text (`RawTextFormField`,
  `flui_material::TextFormField`), and reporting it no longer writes it back over a composition.
- **An application edit made while an input method holds the field's lock** wins: the input
  method's session is dropped and the method hears of the application's edit.
- **`InMemoryTextStore::owner_notifications`** counts sessions that changed the committed text,
  delivered after each lock is released.
- **`TextInputOwner::dispatch` and `run_deferred_grants`** (`flui-interaction`) resume a
  panic a store parked in the presentation's commit gate (a field's `on_changed` failing after an
  input method's grant), so it reaches the realm's report; the grant itself stands.
