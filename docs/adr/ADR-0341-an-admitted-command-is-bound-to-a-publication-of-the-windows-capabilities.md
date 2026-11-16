# ADR-0341: An admitted command is bound to a publication of the window's capabilities

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0254](ADR-0254-window-contract.md) §2 for the binding of an
  `AdmittedCommand`, the variants of `AdmissionError` and the meaning of `ApplyError::Superseded`,
  there and in its Confirmation (the token carried the capabilities revision it was admitted
  against, `apply` compared that revision with the window's current one, and `Superseded` meant the
  revision moved)
- **Related:** [ADR-0313](ADR-0313-a-generational-slot-never-mints-an-id-twice.md) (identities are minted, never built
  from a number), [ADR-0097](ADR-0097-no-process-global-state-gate.md) (the publication counter is a granted
  process global)

## Context and problem statement

ADR-0254 §2 makes admission a type: `WindowCommand::admit(&WindowCapabilities)` is the only
constructor of `AdmittedCommand`, and a window opens the token under its published capabilities.
The token carried those capabilities' revision, and the code added the window's
`CapabilitiesOwner`. Three ways to forge that evidence through the public API all opened on the
window. Each was shown by a test in `flui-platform`'s `window_command_validate` against
`origin/main` at `ab45cd94c`:

1. **An edited copy.** `WindowCapabilities` was `Copy` with `pub` fields. Copying a window's
   `state().capabilities` and setting `material = WindowCapability::Native` produced a value with
   the window's owner and revision, on a window that draws no material. `SetMaterial` was then
   admitted against it, and the window carried it out. The Android window answered `Ok(())` for
   the no-op.
2. **A forged owner.** `CapabilitiesOwner::new(NonZeroU64)` was public, and hosts drew owners from
   a counter starting at 1. `WindowCapabilities::ALL.owned_by(CapabilitiesOwner::new(1))` admitted
   anything on the first window whose capabilities had never changed (revision 0).
3. **A computed successor.** `WindowState::successor` is public, because hosts in other crates
   publish through it. It carries the owner from `self` and sets the capabilities revision to the
   next snapshot revision when they change. A caller who passed the window's snapshot and a sample
   claiming every capability got the window's owner at revision N+1. If the window's own next
   snapshot was a capability change, it landed at N+1 too, and the forged token opened under it.

The question: what does an `AdmittedCommand` prove, and who can construct the evidence it rests
on?

## Decision drivers

- A window carries out only what its own published capabilities honour. A host is never told
  "done" for a command its snapshot says it does not take (ADR-0254 §2 and §6).
- The evidence is unforgeable by type, not by a check a host must remember. Every host opens the
  token through one contract function, and no host implements admission itself.
- A new identity is minted, never built from a number (AGENTS.md "Current forms", ADR-0313).
- `WindowState::successor` and `WindowStateParts` stay public. Hosts outside the contract crate
  publish through them, and recordings mirror the snapshot through the parts.
- The race ADR-0254 §2 names stays a typed answer, not a platform failure: admitted against one
  set of capabilities, applied after a change. A refusal that admitting again cannot fix is told
  apart from one it can.

## Considered options

1. Owner and revision, with the owner minted by the contract, plus a `permits` check in `against`.
2. A publication minted by the contract. Read-only capabilities, and admission refusing
   capabilities of no publication.
3. Capabilities held by the window and never handed out. Admission becomes a window method.

## Decision

Chosen option: 2. The rule it gives: **a command is admitted only against a publication, and a
window opens it only under its own.** Anyone may publish capabilities (`publish` is public, and
every window double uses it), but a publication matches nothing a window holds unless that window
published it. The option also keeps the snapshot as the one place a caller reads what a window
honours.

The shape, in `flui-platform`:

```rust
#[derive(Debug, Clone, Copy)]
pub struct WindowCapabilities {
    parts: CapabilityParts,  // read by value: `capabilities.material()`
    edition: Edition,        // Unpublished | Published(Publication) | Withdrawn
    revision: WindowRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Publication(NonZeroU64); // from one `AtomicIdMint`, never reissued

impl WindowCapabilities {
    pub fn publish(self) -> Result<Self, IdExhausted>;   // a host's first snapshot
    pub const fn into_parts(self) -> CapabilityParts;    // the one way out: unpublished
    pub const fn unavailable(self, ..) -> Self;           // an edit: unpublished
    pub const fn withdraw(self) -> Self;                  // commands absent: withdrawn
    pub fn publish_from(self, mint: &PublicationMint) -> Result<Self, IdExhausted>;
    pub const fn is_withdrawn(&self) -> bool;
    pub const fn material(&self) -> WindowCapability;     // one accessor per capability
}
impl CapabilityParts {
    pub const fn into_capabilities(self) -> WindowCapabilities; // unpublished
}

pub struct AdmittedCommand { command: WindowCommand, publication: Publication }

impl WindowCommand {
    // InvalidGeometry, then Unpublished, then Unsupported.
    pub fn admit(self, capabilities: &WindowCapabilities) -> Result<AdmittedCommand, AdmissionError>;
}
impl AdmittedCommand {
    // Ok only under the very publication it carries; otherwise Superseded { command }.
    pub fn against(self, capabilities: &WindowCapabilities) -> Result<WindowCommand, ApplyError>;
}

#[non_exhaustive]
pub enum AdmissionError { Unsupported(Unsupported), InvalidGeometry(InvalidGeometry), Unpublished }
```

- **Minting.** `publish` mints a publication for a window's first snapshot (`WindowStateCell::open`
  in `flui-native`).
- **Successor.** `WindowState::successor` keeps the publication while the capabilities stay equal.
  When they change, it mints a new one at the new snapshot's revision. A predecessor of no
  publication yields a successor of none.
- **No reader can change what it holds.** A reader can copy a published value but not change it.
  Every edit, every constant and every value built from `CapabilityParts` is unpublished. `admit`
  refuses those with `AdmissionError::Unpublished`, which admitting again never fixes.
- **`Superseded` keeps one meaning.** It means another publication than the token's: the same
  window's later one, another window's, or one the caller published itself. Admitting again
  against the window's snapshot fixes it.
- **No `permits` re-check in `against`.** One publication is one set of capabilities, which no
  reader can change and which `successor` replaces at every change. So the set `admit` checked is
  the set the window holds when the publication matches, and a re-check would ask the same
  question twice.
- **Revision.** The capabilities revision stays a published fact (`WindowCapabilities::revision`).
  It moves at exactly the points where the publication changes.
- **A window with no publication honours no command.** Withdrawn capabilities
  (`WindowCapabilities::withdraw`) set every field a command is admitted against to
  `Absent(CapabilityAbsence::WITHDRAWN)`; `capability` answers the same for the normal mode. The
  reports, observations and verbs no command reads (`reports_presentation`, `frame_rate_hint`,
  `places_at_open`, the `observes_*`) keep their values, because a window keeps serving them.
  - Published capabilities never carry the withdrawn reason, so a withdrawn snapshot always
    differs from the published one before it: equality sees the withdrawal, `state_changed`
    fires, and placement and widgets read that no command is taken.
  - Every token then answers `Superseded`, and admission answers `Unpublished`. A window never
    carries out what it lost because it could not say so.
  - `UnsupportedReason::Withdrawn` names the reason. The agent protocol carries it as the
    recording's absence `withdrawn`, published as protocol 0.15, a listed break (`BREAKING`):
    an older client refuses the unknown value.
- **The two counters fail apart.** Neither wraps.
  - **Publications.** The process's publications come from one `PublicationMint`
    (`PublicationMint::process`). When it has none left and a window's capabilities change,
    `successor` withdraws them. The window goes on publishing its geometry, metrics, focus and
    every other fact at new revisions, and its capabilities stay withdrawn in every later
    snapshot. A window opened after that opens with withdrawn capabilities, and `open` stays
    infallible. Refusing the open is not every host's to do: an Android activity or a UIKit scene
    exists before its window is built.
  - A host takes its mint (`WindowCapabilities::publish_from`, `WindowState::successor_from`),
    and production passes the process's. Behind the `testing` feature,
    `PublicationMint::exhausted` issues nothing, so a test can drive these paths.
  - **Revisions.** The snapshot that takes the last revision is the window's final one. The cell
    publishes it with its capabilities withdrawn, announces it as any change, and freezes. **Frozen
    means unpublished, with no exception**, whatever the sample carried, so a capability lost
    later cannot be carried out.
  - **Revision equality stays exact.** No revision names two snapshots: the withdrawal is the
    snapshot at the last revision, not a second announcement at it. A first snapshot already at
    the last revision opens frozen and withdrawn. `successor` refuses with
    `WindowRevisionExhausted` only for a snapshot already at the last revision.

### Consequences

- Good, because each forgery above now fails: writing a field of an edited copy does not compile,
  while taking the copy apart and building it again (`into_parts`, `into_capabilities`) compiles
  and `admit` refuses the result as `Unpublished` (1); the owner type no longer exists (2); and a
  token admitted against a computed successor never opens (3).
- Good, because `Superseded` keeps one meaning, and `admit_and_apply` no longer spends its one
  re-admission on capabilities that can never be admitted.
- Good, because a host has nothing to remember: it publishes once through the cell and opens
  tokens against its own snapshot, as before.
- Bad, because a window double must publish its capabilities once and open tokens against those.
  A double that admits against `WindowCapabilities::NONE` is refused `Unpublished`.
- Bad, because a capability change costs one atomic compare-and-swap on a process-wide counter.
  Capability changes are rare: a compositor revising what it allows, a window manager restarting.
- Neutral, because equality and hashing still compare what is honoured, not the publication or
  the revision.

### Confirmation

- **Compiler.** `tests/compile_fail/published_capabilities_cannot_be_edited.rs` in
  `flui-platform`: writing a capability of a window's capabilities does not compile, since each
  one is an accessor (E0615), not a field.
- **Compiler.** `tests/compile_fail/admitted_command_has_one_constructor.rs`: a token is built
  only by `admit`.
- **Tests** in `flui-platform`'s `window_command_validate`:
  - `an_edit_of_published_capabilities_is_refused_at_admission`: an edited copy, `unavailable` on
    published capabilities, a value from `CapabilityParts` and the constants are refused
    `Unpublished`. Publishing equal capabilities again is another publication.
  - `a_successor_a_caller_computes_opens_nothing`: a token admitted against a successor a caller
    computed does not open under the window's next capabilities at the same revision.
  - `a_successor_of_unpublished_capabilities_stays_unpublished`.
  - `withdrawn_capabilities_admit_nothing_in_every_later_snapshot`: withdrawn capabilities take
    no command, the normal mode included, keep their reports, and stay withdrawn in a later
    snapshot while its other facts publish.
- **Tests** in `flui-native`'s window state cell:
  - `the_snapshot_that_takes_the_last_revision_withdraws_the_capabilities`: the snapshot at the
    last revision is announced with its commands withdrawn, even though the sample kept them;
    reports keep their values; nothing is admitted or published afterwards.
  - `a_window_opened_at_the_last_revision_is_frozen`.
  - `a_window_opened_with_no_publication_left_publishes_on_withdrawn` and
    `a_change_with_no_publication_left_withdraws_the_capabilities`, with
    `PublicationMint::exhausted`.
- **Conformance**, run by every host the kit runs: `a_window_opens_only_its_own_publication` (a
  command admitted against the window's snapshot opens, one against capabilities the caller
  published answers `Superseded`), and `a_window_opened_after_a_close_has_an_id_of_its_own` (one
  admitted against the closed window's publication answers `Superseded` on the next window). The
  kit's own `a_window_that_republishes_on_every_read_is_caught` shows the first fails on a host
  that publishes anew on every read.
- **Test** in `parts`: `parts_leave_the_revision_and_publication_to_the_publishing_window`.
- **Global state.** `xtask globals` lists `window_capabilities::PUBLICATIONS` under ADR-0097.

## Pros and cons of the options

### 1. Owner and revision, plus a `permits` check in `against`

- Good, because it is the smallest change: mint the owner inside the crate, drop
  `CapabilitiesOwner::new`, and re-check the command against the window's capabilities when
  opening.
- Good, because the check makes a forged token harmless. A window carries out only what its
  capabilities permit.
- Bad, because the evidence stays forgeable. Owner plus revision is reproducible through the public
  `successor`, and an edited copy still admits. The token proves nothing, and the safety rests on
  a second check of the same question.
- Bad, because "admitted against capabilities no window published" still answers `Superseded`. A
  retry cannot fix it, and the retry loop spends a re-admission on it.

### 2. A publication minted by the contract (chosen)

- Good, because the evidence cannot be fabricated. The identity is minted only inside the crate,
  values that carry it are read-only, edits and constants have none, and a window opens only its
  own.
- Good, because `successor` stays public. Its output for a caller is a publication of its own,
  which matches nothing a window holds.
- Bad, because the API changes: field reads become accessor calls, backends build
  `CapabilityParts`, and doubles publish.

### 3. Capabilities held by the window, never handed out

- Good, because no value could be copied or edited, since there is no value.
- Bad, because the snapshot stops being the one source of what a window honours (ADR-0254 §1).
  Recordings, the conformance kit and every caller that asks up front would need a second read
  path.
- Bad, because admission becomes a method of the window. A queue that admits at submit time and
  applies at the owner's next turn (ADR-0237 decision 4) would hold a borrow of the window, or the
  window itself.

## More information

- Neither winit nor GPUI has an admission token, so they have nothing to forge. The token is FLUI's
  stated advantage, which is why the hole was worth closing by type.
- Revisit when a window's capabilities change often enough for one atomic operation per change to
  show in a profile. No host does today.
