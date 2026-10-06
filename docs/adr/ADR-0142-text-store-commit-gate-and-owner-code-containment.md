# ADR-0142: Text store owner notification, commit gate and owner-code containment

- **Status:** Accepted (2026-10-06). Items 1–3 and 8 are implemented; items 4–7 land with the
  Win32 text-services host, the widget's composition handling and the runtime's anchor debt. Of
  those, the host contract, the owner's `complete_composition` with the `Abandoned` path, and the
  removal of `active_store` are in the code
  ([ADR-0135](ADR-0135-win32-text-services-hold-the-text-store-on-the-owner-thread.md)).
- **Date:** 2026-10-06
- **Supersedes:** [ADR-0090](ADR-0090-ime-pull-text-store-contract.md), in part: §1's rule that
  a read-write session is one change notification to the widget (the owner now hears only of a
  change to the committed text, after the lock is released, items 1 and 2); §2's reporting of a
  failure after a projected grant (item 2); §3's platform mapping reading
  `TextInputOwner::active_store` (item 7); and the interim implementation's write-back (item 3).
  ADR-0090's pull contract stands: the store surface, UTF-16 offsets, locks, the commit gate as
  the frame transaction, the push projection, TSF and UIA on Windows, the conformance kit, live
  evidence in exit B1, its divergences and the rest of its platform mapping.
  [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §6, for its blur rule only
  (item 4).
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §3 (the commit anchor),
  [ADR-0123](ADR-0123-exceptional-presentation-close.md) (presentation close keeps its own
  containment), [ADR-0127](ADR-0127-exceptional-path-retention.md) (values retained on an
  exceptional path)

## Context

ADR-0090's store contract let a store run its owner's code inside a platform session:
`EditableText` wrote a session back and called `on_changed` while the lock was still held, so
owner code that asked for a lock was refused, and it compared the whole text, so `on_changed`,
autovalidation and a saved draft saw text the input method was still composing. The store, its
arbiter and the presentation that attaches it also call code they do not control at many points
(a grant's body, `on_changed`, the controller's listeners, the platform's observer, a custom
store's methods, diagnostics, the destruction of captured values), and each point that handled a
panic on its own lost obligations or reported a later failure in place of an earlier one.

This ADR fixes what the owner sees, when it hears of it, and how every call into code the
framework does not control is contained.

## Decision

1. **The owner works with the committed text**: the document with its composing range replaced
   by the text that occupied that range before the composition began — nothing for a new
   preedit, the original words when an input method reconverts text the user already
   committed. The field's owner is told only when the committed text changed; a session that
   only composed, cancelled a composition, or only marked existing text as a composition tells
   it nothing. A store keeps that origin beside its composing range and accounts for a
   session's edits with `text_store::CompositionLedger`, which knows what every character
   stands for: a committed character stands for itself, a character the session inserted
   stands for nothing, and committed text an edit removed from a composition (or from a
   composition an edit cleared) is kept where it was removed. The committed text is every
   visible character, with the composition replaced by what its characters and removals
   stand for. So text the session inserted and then marked is a new preedit, text it found and
   marked is a reconversion, and a composition an edit cleared keeps its origin until it is
   marked again; the text one edit inserted, with what it removed, is one replacement, and
   replacements that rewrote each other's text are one. A session opens with its composition
   standing for itself when the origin is its visible text, as new preedit when the origin is
   empty, and otherwise as one replacement of the origin by the visible text. **Narrowing:**
   when a mark leaves part of a composition (or of a composition an edit cleared) outside the
   new range, that part commits as the user sees it: its characters stand for themselves and its
   removals are dropped. A removal travels with its own replacement's characters: when every
   character a replacement inserted lies outside the new range, the replacement leaves whole,
   its removal with it; when they lie both inside and outside it, its removal leaves (the
   replacement is split). A removal whose replacement inserted no characters (a deletion) stays
   only strictly inside the new range, so a deletion at its edge commits ("abcdefghi" marked whole,
   "def" deleted, narrowed to "abc": the committed text is "abcghi"). A removal that stays sits at
   the composition's edge nearest it. The rest keeps what its own characters and removals stand
   for, unless a replacement with non-empty removed text is split; its removed text cannot be
   divided, so the rest of that region then stands for its own visible text, and the committed
   text over it is what the user sees. A replacement's removed text is therefore never
   counted beside any of its own inserted text (no "abcdefDEF" from narrowing a conversion of
   "abcdef" to "ABC"). Text that never stood for anything (new preedit) commits as shown beside
   the origin the rest keeps. A text form field validates and saves the committed text
   (`TextEditingController::committed_text`).
2. **The owner hears after the lock is released, before the next grant.** `LockArbiter::request`
   and `run_deferred` take a second function, `settle`, called after each grant has released its
   lock and before the next queued grant runs. A store commits the session's result inside the
   grant and records the notification it owes; it delivers it in `settle`, then the observer
   notifications that waited for the lock. A synchronous lock requested from the owner's code is
   granted (after the grants queued ahead of it), so an `on_changed` that edits the field lands
   after the session and reaches the platform as an application edit. A panic in `settle` does
   not undo the grant, which already ran: the arbiter catches it, hands the first payload to the
   store's `CommitGate` (`defer_failure`; later ones are retained per ADR-0127), and the queue
   keeps running; the gate's owner takes it (`take_failure`) and reports it at its next turn
   through the realm's panic report: `TextInputOwner::dispatch` and `run_deferred_grants` take it
   once they return and resume it inside their containment (the first failure stays
   authoritative), so it reaches the realm's report from the dispatch or anchor that follows. A
   store also tells its observer before it resumes an owner panic, so a grant queued behind it
   never runs before the platform hears of an edit the owner made. A store whose owner never
   installed a gate resumes the panic once the lock is released. Under TSF the grant's
   `RequestLock` still returns `S_OK` with `*phrSession` from `OnLockGranted`.
3. **A platform session the application overtook is dropped.** The controller counts its
   changes (a generation); a read-write session records the count when it opens, and if the
   application changed the field before the session writes back (a nested modal loop on the
   owner thread, an async task), the session's result is discarded, the application's edit
   stays, and the platform hears of it through the observer once the lock is released, so the
   text service re-reads the document. There is no merge: nothing gives a base for a three-way
   comparison. The count never wraps: a store that exhausts it drops every later session, so
   no session can find its count again after the application moved past it. The controller and
   the in-memory store share the count's type, `flui_platform_api::text_store::EditGeneration`.
4. **A field losing its input resolves its composition by committing it.** Pointer-down in the
   presentation (inside the composing field too), an accepted close request, the end of the
   session, blur, paste and undo, and unmount commit the visible composition before the event's
   handlers run. A platform that cannot terminate its composition (a refused lock) reports
   `Abandoned`; the field then clears the composing range itself, keeping the text. Unmount
   commits in place and calls no `on_changed`; only an input-method edit not yet applied when the
   store detaches is lost. This supersedes ADR-0030 §6's "blur detaches the IME client but does
   not end the composition". Until this item lands, a form reset during a composition leaves the
   preedit in the field (the reset writes only the committed text it compares against).
5. **A commit anchor skipped by an unwound frame is a debt of the realm**, paid at its next owner
   turn (a drained inbox, a background pump or a pump), not at the next frame, and a failed wake
   does not clear it.
6. **The commit gate is open only while the realm is in its slot and not driving a frame.** A
   platform entry while the realm is checked out (a frame, the end of a session) is refused a
   synchronous lock and queues an asynchronous one; no user code runs. A presentation's close is
   its last turn and opens the gate for good: no anchor follows it, so a completion queued in a
   frame the close cuts short commits (item 4) before its store is retired. An accepted
   composition commit is completed and the rest of the queued tail is cancelled: a pull host's
   queued completion runs through the host (in place if it abandons it), and a store whose
   in-place commit was queued as a grant (behind the shut gate, or behind a grant running on the
   store when the completion was asked for from inside it, which may fail and leave it queued;
   the owner records the debt from the request's outcome, not from the gate) runs its queued
   grants, in request order, so the commit lands behind the grants accepted before it; every
   other store's queued grants are dropped with the store. Each runs inside the close's
   containment, and after a failure the rest are retired, not run.
7. **ADR-0090 §3's platform mapping follows a pull host.** The Win32 backend receives the focused
   store from the presentation through an owner-thread host rather than reading
   `TextInputOwner::active_store`, which is removed.
8. **Owner code runs inside one containment.** Every point where the arbiter, a store or the
   presentation runs code it does not control — a grant's body, a settle, `on_changed`, the
   controller's listeners, an owner listener, the observer (the flush after a request's grants
   included, which yields to a failure their settle parked), `on_session_start`, the projection,
   a store installing a gate, a pull host's focus and completion calls from the presentation's
   queue (ADR-0135 §4), diagnostics (a `tracing` subscriber is user code), and the destruction of
   any snapshot, refused or queued grant, replaced value, client, store or host clone — goes
   through `text_store::OwnerCalls`, whose module doc lists them; a gate whose last clone goes
   with a failure no owner took retains it. What the code is owed (obligations with their values,
   the gate a failure belongs to) is read before it runs, never after, since it may reenter,
   settle a nested session or move the store to another presentation; so the `on_changed` an edit
   is owed to is the one installed when the edit is accepted (a session written back, a key or
   semantic edit before it runs), though the edit's listeners rebuild the field and remove or
   replace it before the owner hears of the change. Each call is contained and
   the first failure in time is authoritative: a settle parks its failure in the admitting gate
   the moment it is caught, ahead of any session the owner's later code opens; a failure a call
   parked in a gate before its own panic came first, so it is taken first, while one the gate
   already held waits for its owner's turn (a dispatch, an anchor, a completion, a close); a
   later one is retained (ADR-0127). The gate records whether the thread was unwinding when a
   failure was parked: one parked while it was (a guard's `Drop` in the call's cleanup requested a
   grant whose settle failed) came after the panic that started that unwind, and is kept behind
   the call's own. When a panic began is not observable from outside it (the panic hook is
   process-global and the application's), so a failure parked during an unwind the call caught
   itself and then outlived is ordered behind the call's panic too; that is the conservative
   order, and both are kept. An operation completes its own state changes before it raises a
   failure it caught: attach admits the client and returns its token, and a failure after that
   point waits in the gate for the owner's next turn unless the owner closed meanwhile; an
   update, a key edit, a semantic edit, the cursor-area loop and dispose each finish their
   steps. Work stays deliverable: an asynchronous request behind a failing queued grant is
   queued, and a synchronous one, or one refused because the flush before it failed, is retained
   rather than destroyed during the unwind. A snapshot retires inside the scope: dropped while
   the scope is healthy, retained once it has failed or while the thread unwinds. Nested work
   runs in the caller's scope, so it sees the caller's failure. A session the field was unmounted
   under is not written back. Presentation close keeps its own containment (ADR-0123) under the
   same retention rule.

## Alternatives considered

- **Notify the owner inside the grant, under the lock.** Rejected: owner code that asks for a
  lock is then refused, and the owner sees text the input method has not committed.
- **Merge an overtaken session with the application's edit.** Rejected: neither side records a
  base, so there is nothing to merge against (item 3).
- **Contain owner code at each point by hand.** Rejected: each point drifted on its own (a later
  failure reported in place of an earlier one, an obligation lost with an unwind); one helper and
  one matrix over every point keep the rule in one place.

## Consequences

- `TextStore` implementors deliver owner notifications from `settle` and run owner code through
  `OwnerCalls`; the conformance kit's version 2 checks the notification rules.
- A failure in owner code after a grant no longer escapes the platform call that granted it: it
  reaches the realm's report from the presentation's next dispatch, anchor or close.
- Values a failed scope retains are leaked by design (ADR-0127).

## Verification

In place:

- Items 1–3: `flui-platform-api` `tests/lock_gate.rs`'s
  `settling_runs_owner_code_outside_the_lock` (settle after each release and before the next
  grant, a panicking settle reaching the gate while the queue drains, and resuming when no owner
  installed a gate); `the_ledger_follows_the_reference`, which checks the ledger against a
  reference model over random edit, mark and session sequences, with the cases that model found
  (`composition_ledger_named_cases`); kit version 2's
  `composition_over_a_selection_replaces_the_selection`,
  `composition_only_sessions_do_not_notify_the_owner`,
  `reconverting_committed_text_notifies_only_on_commit` and
  `owner_notification_runs_after_release`, with `flui-testing`'s
  `conformance_fails_a_store_that_notifies_its_owner_of_a_composition`,
  `conformance_fails_a_store_that_notifies_its_owner_under_the_lock` and
  `a_pinned_conformance_version_does_not_grow`; `flui-widgets`
  `on_changed_runs_after_the_lock_is_released`,
  `an_app_edit_during_a_lock_is_not_overwritten`,
  `swapping_the_controller_during_a_grant_drops_the_session`,
  `a_panicking_on_changed_is_reported_once_and_the_field_keeps_working` (a failure reaching the
  next owner turn once, one raised by the dispatch, the first of two kept, and the observer
  told before the next grant) and `a_text_form_field_validates_and_saves_the_committed_text`.
- Item 8: `flui-widgets` `owner_code_is_contained_at_every_point`, one row per point and failure
  shape, each in a child process, each followed by the next operation on the same owner.

Outstanding:

- Items 4–7, with their tests: the Win32 window offering its text-services host (item 7; the
  host contract and the removal of `active_store` are ADR-0135's), the widget's committing of a
  composition on blur, paste, undo and unmount (item 4), the runtime's pointer-down and close
  hooks, anchor debt and the gate's realm rule (items 4–6), and a failure the gate holds being
  reported through the anchor debt when no dispatch or anchor follows (items 2 and 5).
