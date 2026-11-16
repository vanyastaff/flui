# ADR-0341: An admitted command is bound to a publication of the window's capabilities

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0254](ADR-0254-window-contract.md) §2 for the binding of an
  `AdmittedCommand` only (the token carried the capabilities revision it was admitted against, and
  `apply` compared that revision with the window's current one)
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

Chosen option: 2, because it is the only one where every value a caller can hold either is
exactly what a window published, or cannot be admitted. It also keeps the snapshot as the one
place a caller reads what a window honours.

The shape, in `flui-platform`:

```rust
#[derive(Debug, Clone, Copy)]
pub struct WindowCapabilities {
    parts: CapabilityParts,             // read by value: `capabilities.material()`
    publication: Option<Publication>,   // `None` for anything not published
    revision: WindowRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Publication(NonZeroU64); // from one `AtomicIdMint`, never reissued

impl WindowCapabilities {
    pub fn publish(self) -> Result<Self, IdExhausted>;   // a host's first snapshot
    pub const fn into_parts(self) -> CapabilityParts;    // the one way out: no publication
    pub const fn unavailable(self, ..) -> Self;           // an edit: no publication
    pub const fn withdraw(self) -> Self;                  // a frozen window: no publication
    pub const fn material(&self) -> WindowCapability;     // one accessor per capability
}
impl CapabilityParts {
    pub const fn into_capabilities(self) -> WindowCapabilities; // no publication
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

#[non_exhaustive]
pub enum WindowStateExhausted { Revisions, Publications } // what `successor` refuses with
```

- **Minting.** `publish` mints a publication for a window's first snapshot (`WindowStateCell::open`
  in `flui-native`).
- **Successor.** `WindowState::successor` keeps the publication while the capabilities stay equal.
  When they change, it mints a new one at the new snapshot's revision. A predecessor of no
  publication yields a successor of none.
- **No reader can change what it holds.** A reader can copy a published value but not change it.
  Every edit, every constant and every value built from `CapabilityParts` has no publication.
  `admit` refuses those with `AdmissionError::Unpublished`, which admitting again never fixes.
- **`Superseded` keeps one meaning.** It means another publication than the token's: the same
  window's later one, or another window's. Admitting again against the snapshot fixes it.
- **No `permits` re-check in `against`.** One publication is one set of capabilities, which no
  reader can change and which `successor` replaces at every change. So the set `admit` checked is
  the set the window holds when the publication matches, and a re-check would ask the same
  question twice.
- **Revision.** The capabilities revision stays a published fact (`WindowCapabilities::revision`).
  It moves at exactly the points where the publication changes.
- **Exhaustion** cannot wrap.
  - At `publish` for a window's first snapshot, the window opens frozen and unpublished. Refusing
    the open is not every host's to do: an Android activity or a UIKit scene exists before its
    window is built.
  - In `successor`, it is `WindowStateExhausted::Publications`, and the window publishes nothing
    again, as at its last revision.
  - **A frozen window whose capabilities can no longer be published is unpublished.** When the
    freeze comes from `Publications`, or from `Revisions` with a sample whose capabilities differ,
    the cell withdraws its snapshot's publication (`WindowCapabilities::withdraw`). Every token
    then answers `Superseded`, and admission answers `Unpublished`. A window never carries out what
    it lost because it could not say so.

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
- **Test** in `flui-native`'s window state cell:
  `a_freeze_that_moves_the_capabilities_withdraws_their_publication`.
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
  values that carry it are read-only, and everything else has none and cannot be admitted.
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
