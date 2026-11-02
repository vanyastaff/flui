# ADR-0284: Text-store observers bracket document publication

- **Status:** Accepted (2026-11-02)
- **Date:** 2026-11-02
- **Related:** [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (pull text stores), [ADR-0142](ADR-0142-text-store-commit-gate-and-owner-code-containment.md) (bounded lock requests and owner-code containment), [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (UIKit's shared store host), [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md) (first failure and cleanup)

## Context and problem statement

UIKit's `UITextInputDelegate` receives before and after notifications for an application's
changes to its document. The existing `TextStoreObserver::text_changed` and
`selection_changed` callbacks report changes after publication; sending both UIKit notices
from one such callback lets a synchronous query in the before notice see the new document.
An edit can also wait behind a store lock or commit gate and join other edits, so a controller's
pre-edit callback does not identify the change that the input method will eventually observe.
How does the store expose one publication to an input method without giving the host another
editable document or losing accepted edits when a callback fails?

## Decision drivers

- `flui-platform` owns the store contract; stores and hosts use the same notification boundary.
- Before queries see the previous published document and after queries see the new one.
- A platform's own edits are not echoed to its input method.
- Reentrant writes follow publication; synchronous read queries remain available.
- One observer receives the transaction, even if a callback replaces the installed observer.
- A callback cannot omit or repeat publication, and the first failure survives later callbacks.

## Considered options

1. Send before and after notices from existing post-change callbacks.
2. Tell the host before each controller edit.
3. Give the observer a store-owned publication continuation and a shared failure ledger.

## Decision

Chosen option: 3, because the store knows when an accepted change becomes visible, including
changes coalesced behind a lock or commit gate.

§1. **The platform contract brackets publication.** `TextStoreChange` is a non-exhaustive enum:

```rust
#[non_exhaustive]
pub enum TextStoreChange {
    Text(TextChange), // Also changes the selection.
    Selection,
    Composition, // Composition or its committed-text origin changed.
}

pub trait TextStoreObserver {
    fn publish_change(
        &self,
        change: TextStoreChange,
        publish: &mut dyn FnMut(&mut OwnerCalls),
        calls: &mut OwnerCalls,
    ) {
        publish(calls);
    }
    // Existing text_changed, selection_changed, layout_changed, status_changed.
}
```

The default forwards publication, preserving existing observers. A host uses `calls.run` for
each before notice, calls `publish(calls)`, and keeps the same ledger through after notices,
capture destruction and queued host work. It does not resume an inner failure before the
remaining publication work runs. Text changes bracket both text and selection notices;
selection-only changes bracket selection notices alone.
Composition metadata has its own `Composition` change and matching
`TextStoreObserver::composition_changed` callback; it does not invent an empty text replacement.
Like a text change, it also invalidates selection: its before and after selection notices run
even when the selection's numeric offsets stay the same.

§2. **The store owns read visibility and exactly-once publication.** Its read-only published
document stays at the old version during the before callback, then advances before delivering
the existing after callbacks. Those callbacks use the observer captured for this transaction.
The continuation advances once; a repeated call is inert. If the observer omits the call or
panics before it, the store advances and delivers the after callbacks itself. Read-write lock
requests defer, or receive the existing synchronous refusal, through the normal bounded lock
queue. An application edit made reentrantly becomes a later publication.
`LockArbiter::read_snapshot` admits only the synchronous snapshot read while the arbiter is
unlocked and its commit gate is open. Its RAII guard restores the lock state without settling
or draining queued grants. This is a narrow exception to ordinary FIFO admission: publication
defers asynchronous reads and every write through the existing bounded queue, and a synchronous
write receives the existing typed refusal.
After the after notices, a settling phase executes the accepted publication-time grants that
remain eligible under the current gate and lock state, through the same failure ledger before
`publish` returns. The native host still holds its queue entry until those eligible writes
settle. A closed gate or nested lock leaves the bounded tail deferred for its next real owner
anchor; the drain neither forces admission nor retries a failing batch. An already admitted
publication finishes its paired notifications if a callback closes the gate, while fresh reads
may be refused and fresh writes may be deferred.

§3. **Apple adapters own keyboard policy.** The shared native Apple client holds its host-queue
entry across before notices, publication and after notices. A requested focus change waits
until this interval ends. A private publication-context snapshot identifies the recipient by
`Rc` identity. Replacing or retiring that context invalidates its remaining notices; a newly
bound context never receives an after notice belonging to the previous context. Losing the
recipient does not cancel document publication.

§4. **The first failure remains authoritative.** The store and host pass one `OwnerCalls`
ledger through publication. A failed before notice does not suppress the change or remaining
eligible notices. Captures are withdrawn before being dropped, and destructors after a caught
failure run under containment. Retirement detaches the focused store and context before
callbacks and completes accepted composition obligations in place.
`LockArbiter::run_deferred_contained` contains each eligible grant body and settlement
individually. Its unlocked checkpoint publishes reentrant application changes before a later
platform write can absorb them; admission is checked again after the checkpoint.

### Consequences

- Good, because UIKit reads the document version corresponding to each notice.
- Good, because each host consumes one store-owned boundary instead of mirroring an editor.
- Good, because an absent, failing or reentrant observer cannot silently drop an accepted edit.
- Bad, because stores keep a read-only published document in addition to their editable state;
  publication copies text and associated read data. No performance improvement is claimed.
- Bad, because custom store implementations must honor this boundary to provide before notices.
- Neutral, because actual UIKit/AppKit behavior still needs native runs; cross-compilation and
  shared-client tests establish different evidence.

### Confirmation

- The shared text-store kit's `application_publication_brackets_old_and_new_documents` runs on
  both `InMemoryTextStore` and the widget text store.
- `publication_finishes_accepted_writes_and_keeps_the_first_failure` asserts queued writes
  have run immediately when the continuation returns, before a read can pump the queue.
- `publication_keeps_reentrant_application_edits_before_platform_writes` and
  `publication_finishes_with_its_captured_observer` cover fresh application state and observer
  replacement during a publication.
- `application_notices_bracket_the_document_the_keyboard_reads` queries old and new text and
  selection through the shared Apple client.
- `a_will_notice_keeps_its_recipient_until_publication_or_retirement` covers deferred focus,
  replacement and retirement inside a before notice.
- `a_will_failure_still_publishes_and_preserves_its_first_failure` competes a before failure
  with an after failure and checks publication and the next operation.
- `retirement_delivers_a_completion_accepted_inside_a_notice` proves an accepted deferred
  completion is not cleared by retirement.
- `an_application_commit_with_identical_text_still_ends_marked_text` checks a composition
  commit whose text and selection do not change.

## Pros and cons of the options

### 1. Before and after notices from post-change callbacks

- Good, because it changes only the native adapter.
- Bad, because before queries already see the new document, contrary to UIKit's contract.
- Bad, because a different name or callback order cannot recover the old published version.

### 2. A callback before each controller edit

- Good, because an immediate edit can query its old document before mutation.
- Bad, because locked or gated edits can coalesce, making this callback differ from publication.
- Bad, because it exposes controller transaction details to the host and duplicates store policy.

### 3. Store-owned publication continuation and shared ledger

- Good, because visibility, coalescing and accepted-write obligations stay with the store.
- Good, because one failure ledger preserves callback order across the entire publication.
- Bad, because read-only snapshots and explicit publication state add copying and bookkeeping.

## More information

Apple's [Lower-Level Text-Handling Technologies](https://developer.apple.com/library/archive/documentation/StringsTextFonts/Conceptual/TextAndWebiPhoneOS/LowerLevelText-HandlingTechnologies/LowerLevelText-HandlingTechnologies.html)
describes the before and after delegate notifications for text and selection changes.
Revisit the snapshot representation if measurement shows publication copying materially affects
editor workloads; retain the visibility and exactly-once contract.
