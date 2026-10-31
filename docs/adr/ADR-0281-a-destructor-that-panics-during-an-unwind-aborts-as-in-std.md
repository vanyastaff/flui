# ADR-0281: A destructor that panics during an unwind aborts, as in std

- **Status:** Accepted (2026-10-31). Not implemented: the retention helpers in `flui-foundation::panic` and the crate-local retention sites still leak; see Migration.
- **Date:** 2026-10-31
- **Supersedes:** [ADR-0127](ADR-0127-exceptional-path-retention.md) §2–§4 (retaining user-owned values after a caught failure or while unwinding) and its hit-test metadata retention; [ADR-0119](ADR-0119-inert-panic-payload-retirement.md) in full (retaining opaque payloads); [ADR-0104](ADR-0104-borrowed-notification-and-opaque-panic-retention.md) §2–§4 only where they retain the callback snapshot, the payload or the remaining envelopes; [ADR-0262](ADR-0262-persisted-documents-over-a-byte-storage-capability.md) §4 and [ADR-0272](ADR-0272-widgets-run-background-work-through-a-capability.md) §3–§4 only where they retain a payload, a capture or a failed future.
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

§2. **Panic payloads are dropped normally.** A payload that is not propagated with
`resume_unwind` is dropped at the place that decided not to propagate it. `payload_text` keeps
borrowing it for diagnostics. `retain_opaque_payload` is deleted, and `FirstPanic::fold` drops a
later payload instead of retaining it.

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
- `PanicRecovery::retire` and `RecoveryScope::retire` have no `mem::forget`. A test in
  `crates/flui-foundation/tests/` drops a retired value after a caught failure and keeps the
  first failure when its destructor panics.
- Until the migration ends, `rg 'thread::panicking\(\)' crates packages src` counts the sites
  still to remove. A new one is a review finding against this ADR.

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

## Migration

The decision is the rule now. A crate-local retention site that has not been removed yet only
leaks on a path that already failed: it is redundant, not a second defect. The sites are removed
one crate at a time, from the bottom of the tiers up:

1. `flui-foundation::panic`: delete `retain_opaque_payload`, make `FirstPanic::fold` drop and
   make the three `retire` methods contain the destructor. Each caller of `retain_opaque_payload`
   drops its payload instead.
2. Each crate removes its `thread::panicking()` branches and `mem::forget` of user-owned values
   in the same change that deletes its child-process double-panic tests.
3. `docs/PANIC-POLICY.md` loses the retention paragraph once the last crate is done.

## More information

- A destructor that panics while the thread unwinds makes the Rust runtime abort with "panic in
  a destructor during cleanup"; `std::thread::panicking` is how a destructor can tell it is in
  that state.
- The review-finding measurement behind the Context is a project retrospective kept outside the
  repository.
- Revisit if a platform requires FLUI to survive arbitrary application destructors (a plug-in
  host loading untrusted code in-process); that would be a process boundary, not retention.
