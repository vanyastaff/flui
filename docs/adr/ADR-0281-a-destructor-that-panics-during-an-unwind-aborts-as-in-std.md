# ADR-0281: A destructor that panics during an unwind aborts, as in std

- **Status:** Accepted (2026-10-31)
- **Date:** 2026-10-31
- **Supersedes:** [ADR-0127](ADR-0127-exceptional-path-retention.md) §2–§4 (retaining user-owned values after a caught failure or while unwinding) and its hit-test metadata retention; [ADR-0119](ADR-0119-inert-panic-payload-retirement.md) in full (retaining opaque payloads); [ADR-0104](ADR-0104-borrowed-notification-and-opaque-panic-retention.md) §2–§4 only where they retain the callback snapshot, the payload or the remaining envelopes; [ADR-0262](ADR-0262-persisted-documents-over-a-byte-storage-capability.md) §4, [ADR-0272](ADR-0272-widgets-run-background-work-through-a-capability.md) §3–§4 only where they retain a payload, a capture or a failed future; [ADR-0166](ADR-0166-combined-viewer-gestures.md) only where native ticket callbacks and captures are retained after a failure or during a preserving close; [ADR-0106](ADR-0106-ticker-continuation-ownership-and-terminal-waiters.md) decisions 4–5 only where they retain exceptional envelopes, secondary payloads and captures; [ADR-0109](ADR-0109-notification-channel-surface.md) decisions 3–4 only where they retain the failed snapshot, the remaining envelopes and a discarded payload (`retain_opaque_payload`); [ADR-0117](ADR-0117-async-snapshot-publication-retirement.md) only its sentence retaining a competing wake payload without its destructor; [ADR-0123](ADR-0123-exceptional-presentation-close.md) only preserving mode's retention of the closing presentation's user-owned values; [ADR-0135](ADR-0135-win32-text-services-hold-the-text-store-on-the-owner-thread.md) §4 only its clause retaining the completed store once the scope has failed; [ADR-0136](ADR-0136-owner-local-ui-surfaces.md) §2 only where teardown retains later panics and, during an unwind, the values; [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md) only where an earlier failure or an unwind retains the outgoing registry or opaque captures; [ADR-0178](ADR-0178-notification-first-failure-custody.md) only where opaque payloads, custody and outgoing ownership are retained after a failure; [ADR-0182](ADR-0182-complete-owner-execution.md) only where cleanup retains the produced frame result after a failure, and its paragraph calling exceptional retention deliberate leakage; [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) §1, §2 and §5 only where they retain a payload whose destructor panics, what a non-terminal close has not destroyed, a later or opaque terminal payload, and a user `Drop` during an unwind; [ADR-0142](ADR-0142-text-store-commit-gate-and-owner-code-containment.md) items 2 and 8 and its consequences only where they retain a later failure or a failed scope's values; [ADR-0158](ADR-0158-hit-path-cursor-and-offset-admission.md), [ADR-0159](ADR-0159-gesture-arena-owner-delivery.md), [ADR-0161](ADR-0161-immutable-owner-local-gesture-recognizers.md), [ADR-0168](ADR-0168-descendant-reveal-through-scroll-ancestors.md), [ADR-0177](ADR-0177-controller-ordered-delivery.md), [ADR-0195](ADR-0195-presentation-motion-policy.md) and [ADR-0223](ADR-0223-authored-semantics-regions.md) only where they retain captures, callback ownership, payloads, snapshots or callback owners after a failure under ADR-0127.
- **Revised:** 2026-11-03 — the Supersedes list reaches every record that prescribed retention; the Migration section is gone, since no retention site is left, and Confirmation names the lint that keeps it so.
- **Related:** [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) (the boundary where a contained user panic ends up)

## Context and problem statement

Rust aborts the process when a destructor panics while the thread is already unwinding, and a
`Vec`, `HashMap` or `Rc` in std accepts that: the destructors of its elements are trusted not to
panic during an unwind. FLUI went further. ADR-0104, ADR-0119 and ADR-0127 make every framework
container leak (`std::mem::forget`) user-owned values and opaque panic payloads, both once any
failure in the operation has been caught and while the thread is panicking, so that a destructor
that panics a second time cannot abort.

The guarantee is written by hand at every place where a framework container drops something.
On 2026-10-31 the workspace had 197 `thread::panicking()` checks in 92 files, `retain_opaque_payload`
called from 105 files, and four retention paths in `flui-foundation::panic` (`retain_opaque_payload`,
`FirstPanic::fold`, `PanicRecovery::retire`/`retire_classified`, `RecoveryScope::retire`). It is
also the largest single class of review findings. In the two weeks of reviews to that date,
roughly one finding in five was a place where a capture, a waker, a resolver or a payload could
be dropped during an unwind or after a caught failure, each found one site per round and fixed one
site per commit. The defence also costs at run time: a retained capture never closes its channel,
and a retained render object keeps its resources until exit.

What does FLUI promise when a destructor panics during an unwind?

## Decision drivers

- A user panic in a callback is contained, and the runtime goes on (ADR-0258). That promise
  stays.
- A destructor that panics during an unwind already breaks a std invariant. Libraries do not
  defend against it, and a value whose own fields panic twice aborts before any FLUI boundary
  regains control anyway (ADR-0104 §3, ADR-0127 "Consequences").
- Rules are types, a lint or a gate (AGENTS.md). A rule that has to be rewritten at every drop
  site, and that review keeps finding missing, is the wrong shape.
- Leaks on the failure path hide resources (channels, GPU objects, file handles) from the
  owner that should release them.

## Considered options

1. Keep the retention of ADR-0127 and enforce it with a wrapper type.
2. Std semantics: a destructor that panics during an unwind aborts. Destroy values normally,
   with each destructor after a caught failure run under the operation's containment.
3. Retain only opaque payloads (ADR-0119) and drop captures normally.

## Decision

Chosen option: 2, because it matches std, removes a defence that cannot be complete anyway, and
turns a rule written at every drop site into ordinary destruction.

§1. **A destructor that panics while the thread is unwinding aborts the process.** FLUI does not
retain (`std::mem::forget`, `ManuallyDrop` without a release) a user-owned value or a panic
payload to prevent it. `std::thread::panicking()` is no longer a reason to skip a destructor.

§2. **Panic payloads are destroyed, under containment.** A payload that is not propagated with
`resume_unwind` is destroyed at the place that decided not to propagate it, with
`flui_foundation::panic::discard_payload`: the failure is already caught, so the payload's
destructor is user code after a caught failure (§3), and a panic it raises is discarded the same
way instead of escaping the boundary that caught the first one. `payload_text` keeps borrowing
it for diagnostics. `retain_opaque_payload` is deleted, and `FirstPanic::fold` discards a later
payload instead of retaining it.

§3. **After a caught failure, a container keeps destroying what it withdrew.** It still
withdraws each value and releases its borrows and locks first (ADR-0127 §1 stands). Each
destructor that can run user code runs under the operation's containment (`FirstPanic::contain`,
`PanicRecovery::run`): a destructor that panics there is a competing failure, and the first
failure stays the one reported or resumed. `PanicRecovery::retire`, `retire_classified` and
`RecoveryScope::retire` run the destructor under that containment instead of forgetting the
value.

§4. **What stays.** The first failure of an operation is the one that propagates (ADR-0104 §2,
ADR-0127 §2's last sentence). Delivery to the remaining receivers continues after a caught
callback panic. Values are withdrawn before their destructor runs. A contained user panic ends at
the owner-turn boundary of ADR-0258.

### Consequences

- Good, because the drop sites lose their per-site branch (`if first.is_some() ||
  thread::panicking() { forget } else { drop }`) and the review class that came with it.
- Good, because values on a failed path release their resources: a channel closes, a GPU
  resource returns to its owner.
- Good, because the guarantee is the one Rust users already know from std.
- Bad, because an application whose destructor panics during an unwind loses the process
  instead of the frame. ADR-0258 already ends the process when nothing is left to continue.
  This adds a case where a hostile destructor decides that.
- Bad, because tests that ran in a child process to show a double panic does not abort lose
  their subject. They are deleted, and the tests of the first failure and the next healthy
  operation stay.
- Neutral, because a destructor that panics outside an unwind is still contained like any user
  panic (§3).

### Confirmation

- `flui_foundation::panic::retain_opaque_payload` no longer exists, so a caller does not compile.
- No crate retains a user-owned value or a panic payload on a failed path, and clippy's
  `mem_forget` (`[workspace.lints.clippy]`) keeps it so: every `mem::forget` carries
  `#[expect(clippy::mem_forget, reason = "…")]` naming why that leak is deliberate (an OS that
  still owns the memory, a destructor that would recurse), so a forgotten payload or capture is a
  lint finding.
- `PanicRecovery::retire` and `RecoveryScope::retire` have no `mem::forget`. The table test
  `recovery_preserves_provenance_and_owned_failure` in `flui-foundation` (row "retirement
  after a caught failure") drops a retired value after a caught failure and keeps the first
  failure when its destructor panics; row "later payload with a panicking destructor" discards a
  payload whose destructor panics and still runs the next callback.
- `std::thread::panicking()` never decides whether a value is destroyed. It only makes an
  incoming unwind count as a held failure: a failure caught during it is discarded instead of
  resumed out of a destructor, where `resume_unwind` would abort (`FirstPanic::resume`,
  `PanicRecovery::new`), and optional callbacks are skipped (`flui-interaction`'s `ClosePanic`,
  `flui-runtime`'s `RecoveryState`). A read that guards a `mem::forget` or skips a destructor is
  a review finding against this ADR.

## Pros and cons of the options

### 1. Keep retention, enforce it with a wrapper type

- Good, because no application can abort through a double panic that reaches a FLUI container.
- Bad, because the guarantee is partial: an aggregate whose own fields panic twice still aborts
  before FLUI regains control.
- Bad, because every container, waker slot and resolver needs the wrapper, and every failed
  operation leaks.
- Bad, because the class keeps generating review findings until the last site is wrapped.

### 2. Std semantics (chosen)

- Good, because it removes code instead of adding it, and the remaining rule (§3) is the same
  containment the operation already has for callbacks.
- Bad, because a hostile destructor during an unwind aborts.

### 3. Retain payloads only

- Good, because the payload is the one value FLUI drops on every failure path.
- Bad, because it keeps `retain_opaque_payload` and a leak per failure for a case std does not
  defend either, and captures get the same treatment as option 2 anyway.

## More information

- A destructor that panics while the thread unwinds makes the Rust runtime abort with "panic in
  a destructor during cleanup"; `std::thread::panicking` is how a destructor can tell it is in
  that state.
- The review-finding measurement behind the Context is a project retrospective kept outside the
  repository.
- Revisit if a platform requires FLUI to survive arbitrary application destructors (a plug-in
  host loading untrusted code in-process); that would be a process boundary, not retention.
