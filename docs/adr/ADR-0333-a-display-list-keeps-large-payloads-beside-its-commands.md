# ADR-0333: A display list keeps large payloads beside its commands

- **Status:** Accepted (2026-11-14)
- **Date:** 2026-11-14
- **Supersedes:** [ADR-0306](ADR-0306-display-list-commands-hold-local-geometry-under-interned-transforms.md)
  §3, in part: "fat payloads are boxed" and "`Path` is one pointer", with the `size_of::<Path>()`
  assertion. §3's two other assertions (`DrawOp` at most 112 bytes, `RecordedOp` at most 120)
  stand, as do §1, §2 and §4–§7.
- **Related:** [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md)
  §6 (a repaint records into the chunk a completion handed back, whose room this decision keeps);
  [ADR-0115](ADR-0115-image-source-affine-paint-boundary.md) (the image-region placement);
  [ADR-0322](ADR-0322-a-control-mounts-one-element-for-its-behaviours.md) (the control-cost rows
  that measure a cold mount)
- **Sources:** Flutter 3.27.0 `DisplayListBuilder` and `DlOpRecord`
  (<https://github.com/flutter/flutter/blob/3.27.0/engine/src/flutter/display_list/display_list.h>):
  ops are records in the list's own storage, reused when the builder is reset, and a path op
  holds a shared path handle.

## Context and problem statement

ADR-0306 §3 sized a recorded command at 120 bytes and reached it by boxing the three payloads
larger than the rest (`DRRect`'s two rounded rectangles, an image region's placement, an image
filter) and by making `Path` one pointer. On `main` none of that was built: `DrawOp` was 216 bytes
and a command 224 (`docs/gaps.md`). Measured, `Path` is 144 bytes (its curves behind a
`OnceLock<Arc<_>>`, its shape hint, a pending offset and two flags), `RRect` 96, the pair of rounded
rectangles 192, an image region's placement 192 and a `ColorFilter` 84.

A box per op allocates on every recording, not only on the first: a retained chunk recorded again
into its own room (ADR-0304 §6) would still ask the heap for one block per path, ring, region and
filter on each repaint. Measured on the box form, the material control rows rose by one or two
allocations each. A `Path` behind one `Arc` allocates for every shape built from a hint
(`Path::from_rrect`, a shadow's silhouette), which today allocates nothing until read.

## Decision drivers

- A command is at most 120 bytes and the op at most 112 (render-scaling gate G7).
- A repaint into a warm list allocates nothing.
- No retained-scene baseline rises; a cold mount costs what it must and no more.
- No `unsafe` container in `flui-painting` for a storage layout.

## Considered options

1. A `Box` per large payload and `Path` as one `Arc` (ADR-0306 §3).
2. A payload buffer beside the commands, one `Vec` per list, ids in the op (chosen).
3. As 2, with the first payload held inline in the list (`SmallVec<[_; 1]>`).
4. Commands and payloads in one allocation: variable-length records in one byte buffer, or the
   payloads growing from the far end of the commands' block.

## Decision

The list keeps its large payloads in a buffer of its own, which it clears with its commands and
whose room it keeps; an op holds a typed `u32` index into it.

```rust
pub struct PayloadId<T> { index: u32, kind: PhantomData<fn() -> T> }   // Copy, Eq
pub trait Payload: private::Sealed {}            // Path, NestedRRects, ImageRegionPlacement, ColorFilter

pub enum DrawOp {
    Path { path: PayloadId<Path>, paint: Arc<Paint> },
    ClipPath { path: PayloadId<Path>, clip_op: ClipOp, clip_behavior: Clip },
    Shadow { path: PayloadId<Path>, color: Color, elevation: f64 },
    DRRect { rrects: PayloadId<NestedRRects>, paint: Arc<Paint> },
    ImageRegion { image: Image, region: PayloadId<ImageRegionPlacement>, paint: Option<Arc<Paint>> },
    ImageFiltered { image: Image, dst: Rect<f64>, filter: PayloadId<ColorFilter>, paint: Option<Arc<Paint>> },
    // … every other variant unchanged
}

pub struct DisplayList { /* … */ payloads: Vec<StoredPayload> /* private */ }

impl<'a> DrawCommand<'a> {
    pub fn payload<T: Payload>(&self, id: PayloadId<T>) -> &'a T;          // the id its op holds
    pub fn try_payload<T: Payload>(&self, id: PayloadId<T>) -> Option<&'a T>;
    pub fn to_buf(&self) -> DrawCommandBuf;      // an owned command with its own payloads
}

const _: () = assert!(size_of::<DrawOp>() <= 112);
const _: () = assert!(size_of::<RecordedOp>() <= 120);
```

- A payload is pushed only when its op is accepted; a refused op (ADR-0306 §4) drops it.
- A payload known to equal the last one stored without building anything (the same `Arc` of
  curves, or the same constructor shape and offset, fill type and finiteness; a `Copy` payload
  by value) is not stored again: the op names that entry, so a shape shadowed and then filled is
  kept once.
- `append` moves the other list's payloads after its own and adds that base to each moved op's
  ids; an empty receiver takes both buffers as they are.
- `clear` drops the payloads before the commands: a payload runs no user code, and a command's
  destructor that panics still leaves an empty list (ADR-0306 §7).
- `Path` keeps its shape: unread shapes stay free.

### Consequences

- Good, because a command is 120 bytes and the op 112, with nothing boxed: the assertions hold.
- Good, because a repaint into a warm list allocates nothing for its payloads, as it did when they
  were inline and as option 1 cannot (`recording_contract::re_recording_a_warm_list_with_payloads_allocates_nothing`).
- Good, because the retained-scene costs fall, measured over the boxless inline form: the
  `allocated_bytes` of `list_10k_scroll_one_screen` by 1.4 %, of `full_reassemble` by 4.6 %, of
  `label_change_*` by 1.9 % and of `caret_blink_in_10k_list` by 14 %; the control rows' bytes fall
  3 to 12 %, and the tab bars and the navigation bar take fewer allocations.
- Bad, because a cold list asks for one block the first time it stores a payload: a control whose
  chunk records a shadow or a ring takes one more allocation to mount (one for the checkbox,
  switch and elevated button rows, two for the chip and filter chip rows), and the list is 24 bytes
  larger (80 bytes more for a list tile, 64 for an elevated button).
- Bad, because a reader resolves an id through the command it came with: a payload is not a field
  of the op, and an op moved out of its list (`DrawOp` clone) names nothing until it is read with
  `DrawCommandBuf`.
- Bad, because an id is a position, not a key: a command refuses an id its op does not hold
  (`try_payload` answers `None`), but an id copied from another list that names the same position
  as its own op's reads this list's entry.
- Neutral, because a list that recorded a heavy frame keeps its payload room, as it keeps its
  commands' room (ADR-0304 §6).

### Confirmation

- The compiler: the two `const` assertions; `StoredPayload` and `Payload`'s sealing trait are
  private, so a payload is reached only through a command; `DrawOp::map_payloads`,
  `DrawOp::payload_index` and `DrawOp::is_finite` match with no wildcard.
- `recording_contract` (`crates/flui-painting/tests/main.rs`):
  `re_recording_a_warm_list_with_payloads_allocates_nothing` (counts the blocks a second recording
  into a cleared list asks for: zero; it fails when `clear` lets the payload room go) and
  `appended_payloads_follow_their_commands` (the shared card is one entry; after `append` and
  `append_isolated`, into a list with entries, one with only a transform and an empty one, every
  command reads the payload it was recorded with, and an id another command holds reads nothing).
- The control-cost rows (`packages/*/tests/perf.rs`) and `cargo xtask perf --check` hold the
  numbers above.

## Pros and cons of the options

### 1. A `Box` per large payload and `Path` as one `Arc`

- Good, because a payload is a field of its op, so a reader needs nothing beside it.
- Bad, because every recording allocates one block per large op, a warm repaint included:
  measured, the checkbox, switch, chip, filter chip and elevated button rows rose by one or two.
- Bad, because a `Path` built from a hint allocates its `Arc` at once.

### 2. A payload buffer beside the commands (chosen)

- Good, because the buffer is reused with the list, so a warm repaint allocates nothing.
- Good, because the list grows by one `Vec` (24 bytes), so scenes of many small lists stay cheap.
- Bad, because the first payload of a cold list costs one block.

### 3. As 2, with the first payload inline

- Good, because no control row rises: the one payload a control's chunk holds needs no block.
- Bad, because a payload slot is 200 bytes and every list pays it: `DisplayList` 336 → 520 bytes
  and `list_10k_scroll_one_screen.allocated_bytes` rises by 1.9 %, against option 2's fall of 1.4 %.

### 4. Commands and payloads in one allocation

- Good, because it is Flutter's layout and has option 2's warm cost with no cold block.
- Bad, because it needs an `unsafe` container (typed reads at offsets, a `Drop` for the `Arc` in a
  path, growth that moves both), each with its `SAFETY:` invariant and a `miri` run, for one
  allocation per cold list.

## More information

- Option 4 stays open as its own task in the render-scaling work: it is the form to take if a
  cold mount's allocation count is to fall further.
