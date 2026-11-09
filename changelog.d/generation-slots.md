### Added

- **The slot generations ledger** (`flui-foundation`): `generation::Generation` (a checked,
  1-based, non-zero `u32`; `next` refuses past the last, so no generation wraps) and
  `generation::GenSlots`, the bookkeeping under every packed generational key: `mint_fresh`,
  `mint_recycled` (one checked step at the claim), `generation`, and the release's `peek`→`commit`
  pair (`Release` carries the step; a caught panic between the two steps nothing). A slot whose
  generation space is exhausted is closed at its release (ADR-0313; [Accepted ADR sections not built yet](/docs/gaps.md#accepted-adr-sections-not-built-yet) names the
  render slab's part).

### Changed

- **The reactive signal graph and the element tree mint their generations through the ledger**
  (`flui-view`): the node's bare `u32` generation and the element tree's raw `NonZeroU32` slab
  counters are `GenSlots` values. Visible in traces: a fresh signal slot's generation is now 1
  (it was 0); every compare and refusal is unchanged.