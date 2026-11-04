### Added

- **`AtomicIdMint`** (`flui-foundation`): the cross-thread twin of `IdMint`, with the same sealed
  `MintedId` contract — increasing ids, permanent typed refusal past the id type's own ceiling,
  no wrapping and no reissue.

### Changed

- **Listener, task and callback ids are minted by the `IdMint` family** (`flui-foundation`,
  `flui-scheduler`, `flui-animation`, `flui-interaction`): `ListenerId` comes from the listener
  list's own `IdMint`; the scheduler's `FrameCallbackId` and `TaskId` come from `AtomicIdMint`s.
  The observable sequences are unchanged: every id was already 1-based, increasing and never
  reused; only the exhaustion refusal becomes typed (`IdExhausted`) and the mint becomes the
  workspace's only one.
- **`IdMint` caps itself at its id type's width**: a minted id's raw value stays convertible to
  the identity type's representation on every target, because the mint ends at
  `MintedId::MINT_CEILING` and `ending_at` narrows only.

### Removed

- **`IdGenerator`** (`flui-scheduler`), including `reset`: a mint that could hand an identity twice
  under one address. Refusal is permanent; the scheduler's minting sites name the invariant they
  breach if it ever fires.
- **`flui_foundation::FrameId`**: the frame's identity is the scheduler's 64-bit `FrameId`
  (`flui_scheduler::FrameId`); the foundation alias named a second definition of it that nothing
  minted.