# ADR-0306: Display-list commands hold local geometry under interned transforms

- **Status:** Accepted (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes:** [ADR-0066](ADR-0066-display-list-command-representation.md) "The command is
  `{ transform, op }`" and "The op has a budget" only (the absolute matrix stored in every command,
  `draw_picture` as a per-command re-stamp, and the unasserted 128- and 192-byte budget with a
  32-byte `Path`); its "Gradients are shader paints" and "Serde is off the wire" stand.
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md) §2 (kept: the list records `f64`
  and the engine narrows once, after composing);
  [ADR-0304](ADR-0304-the-scene-is-retained-and-damage-is-the-footprint-of-its-patch.md) (a retained
  chunk holds one render object's list, and its recording cache keys on the composed placement);
  [ADR-0092](ADR-0092-per-realm-text-over-parley.md) §4 (the paragraph payload, unchanged);
  [ADR-0115](ADR-0115-image-source-affine-paint-boundary.md) (the image-region op, now boxed);
  [ADR-0149](ADR-0149-interpolation-contracts.md) §1 (an interpolation carries NaN into geometry);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) §1 (state is committed
  before user code runs); [ADR-0006](ADR-0006-c-ir-record-replay-seam.md) §4 (the one
  `Matrix4`-to-glam edge, now crossed once per interned transform)

## Context and problem statement

A `DisplayList` is what a `Canvas` records and `flui-engine` replays; a retained scene (ADR-0304)
keeps the lists of each render object's chunks across frames. ADR-0066 stored the full transform
in every command and promised a size nothing asserts. The scope is `flui-painting`'s display list
and the readers of it in `flui-rendering` and `flui-engine`. At `e5ba8fe8f`:

| Fact | Evidence |
|---|---|
| A command is `{ pub transform: Matrix4, pub op: DrawOp }`, stamped with the whole CTM by `Canvas::record` | `crates/flui-painting/src/display_list/command.rs:29-35`, `crates/flui-painting/src/canvas/mod.rs:77-84` |
| `Matrix4` is `[f64; 16]`, 128 bytes; `RRect` is a `Rect<f64>` and four `Radius<f64>`, 96 bytes | `crates/flui-foundation/src/geometry/matrix4.rs:124-127`, `crates/flui-foundation/src/geometry/rrect.rs:14-19`, `:135-146` |
| `Path` is an `Arc<BezPath>`, a shape hint (104 bytes, since it can hold an `RRect`), a fill type and cached `Option<Rect<f64>>` bounds: 160 bytes, not ADR-0066's 32 | `crates/flui-painting/src/paint/path.rs:47-52`, `:75-87` |
| The largest payloads: `ImageRegion` 216 bytes (an `Image`, three rectangles and an inline `Option<ColorFilter>` of 84), `DRRect` 200, `Shadow` 184, `Path` and `ClipPath` 168, `ImageFiltered` 144; then `RRect`, `ClipRRect`, `ClipRSuperellipse`, `Vertices` and `Atlas` at 104 | `command.rs:186-202`, `:284-291`, `:257-264`, `:149-154`, `:94-101`, `:229-238`, `:123-128`, `:305-316`, `:333-346`; `crates/flui-painting/src/paint/image.rs:27-37`, `:442-476`; `crates/flui-painting/src/paint/effects.rs:111-114` |
| `DrawOp` is therefore 216 to 224 bytes (by where the compiler puts the discriminant) and a command 344 to 352, against ADR-0066's 128 and 192; 200,000 recorded operations hold about 70 MB of commands. No `size_of` assertion exists, and the crate's architecture document still states 128, 192 and a `Vec`-backed path | `rg 'size_of::<(DrawOp\|DrawCommand)>'` finds none; `crates/flui-painting/ARCHITECTURE.md:55-57` |
| Render objects already draw in local coordinates: the paint recorder translates each fresh canvas to the render object's origin in its layer, so the origin reaches every command through that command's own copy of the CTM | `crates/flui-rendering/src/context/paint.rs:300-308`; `crates/flui-rendering/src/pipeline/owner/paint.rs:771-772`, `:874-882` |
| The runs of inline render objects merge into one picture per boundary, so nothing holds a render object's position once | `paint.rs:804`, `:1501-1503` |
| The engine reinstalls the CTM unless the matrix equals the previous command's (16 comparisons per command); it composes in `f64` and narrows to `f32` after composing | `crates/flui-engine/src/layer_dispatcher.rs:134-161`; `crates/flui-engine/src/batches/shapes.rs:35`, `:60`, `:69`; `crates/flui-engine/src/batches/text.rs:67-83` |
| A non-finite rectangle is recorded as given; the engine refuses it with `GeometryError::NonFinite`, and the layer walk returns that error, so the frame is not drawn | `drawing.rs:42-46`; `crates/flui-engine/src/clip_geometry.rs:257-275`; `crates/flui-engine/src/layer_walk.rs:105-108`, `:186` |
| `Canvas::draw_picture` has no production caller: one bench and one engine readback test | `drawing.rs:358-380`; `crates/flui-painting/benches/display_list_record.rs:82-95`; `crates/flui-engine/src/clip_layer_readback_tests.rs:726` |

How is a recorded command represented so that its size is a checked bound, a render object's
position is held by one transform rather than by each of its commands, and the geometry keeps the
precision the engine's narrowing relies on?

## Decision drivers

- Rules are types (`AGENTS.md`): a size budget is a build failure, not a sentence that drifts.
- A retained chunk (ADR-0304) depends on where its render object sits only through a transform.
- One narrowing point, in the engine, after composing (ADR-0098 §2): only there is the whole
  device scale known, an ancestor's zoom included.
- No non-finite geometry is published, and one painter's bad value does not fail the frame
  (`AGENTS.md`, "Recurring defects").
- The vocabulary stays one closed enum the engine matches without a wildcard (`command.rs:1-7`).
- An index into one list's table means nothing in another list, and the type keeps it inside its
  list.

## Considered options

1. Keep the shape and assert its size.
2. Interned transforms, boxed fat payloads and `f64` geometry, with a list's space defined as its
   layer's: every transform the reader yields is absolute in the layer.
3. As 2, with a list's space its own: the reader yields transforms in the list's space, an op never
   holds its render object's position, and whoever replays the list composes its placement.
4. Local geometry in `f32` under interned `f64` transforms, with a coordinate bound and rebasing.

## Decision

Chosen option: 3, because what the retained scene needs, a render object's position held by a
transform and ops that do not change when it moves, comes from local geometry under an interned
transform, which options 3 and 4 share. `f32` storage saves 48 more bytes per command and costs a
precision bound the recorder cannot choose, a rebasing that has to move values outside the op, and
a second narrowing point against ADR-0098 §2 (Pros and cons, option 4).

§1. **A command is a list-private transform id and an op.**

```rust
// crates/flui-painting/src/display_list/mod.rs: only `DisplayList` leaves the module
#[derive(Clone, Copy)]
struct TransformId(Option<NonZeroUsize>);   // None: the identity, which takes no entry;
                                            // Some(n): transforms[n - 1]
#[derive(Clone)]
struct RecordedOp { transform: TransformId, op: DrawOp }

pub struct DisplayList {
    transforms: Vec<Matrix4>,               // finite, none of them the identity
    commands: Vec<RecordedOp>,
    bounds: Option<Rect<f64>>,
    damage: Option<DamageExtent>,
    volatile: Option<DamageExtent>,
    non_finite: NonFiniteOps,               // §4
}

impl DisplayList {
    pub(crate) fn record(&mut self, ctm: &Matrix4, op: DrawOp);   // the canvas's one way in
}
```

`record` interns the CTM: the identity (`*ctm == Matrix4::IDENTITY`, exact, never the 1e-5
`is_identity` of `matrix4.rs:376`) takes no entry, a CTM equal to the last entry reuses it, and
any other pushes one. `Matrix4`'s `==` is exact (`matrix4.rs:812-818`) and is applied only to
finite matrices, so it is reflexive on everything it compares. The ops of one run share one entry. `append` rebases each
appended id by the receiving table's length with `checked_add(..).expect("BUG: two transform
tables fit in the address space")`, since each table holds at most `isize::MAX / 128` entries, and
keeps its swap when the receiver is empty; `append_isolated` brackets the run with `Save` and
`Restore` under the identity. No public item names `TransformId` or `RecordedOp`, so the ids of two
lists cannot be compared or mixed. `record`, `append`, `append_isolated` and `clear` each update all
six fields.

§2. **Geometry is local and `f64`; no op holds its render object's position.** `DrawOp`'s geometry
fields keep their types (`Rect<f64>`, `RRect`, `RSuperellipse`, `Point<f64>`, `Offset<f64>`,
`f64`): no `f32` geometry type and no local twin of a geometry type exists, the engine stays the one
narrowing point, and the compile-fail fixtures (`crates/flui-painting/tests/compile_fail/`) keep
their meaning. An op holds what the painter drew, in the painter's coordinates. `PaintContext` exposes
no position of its render object except `paint_anchor()` (`paint.rs:410-713`), so the origin
reaches a list only through a transform: the recorder's translation of a fresh canvas
(`paint.rs:300-308`), which interns it as the run's one entry, or a placement its consumer
composes at replay. Neither changes `Canvas`'s public API, which stays `f64` logical. A list holds
no state from outside its recording: the clip, transform and opacity above it belong to whoever
replays it; what the engine bakes from them into its own IR is the engine's (ADR-0006), not this
record's.

`Paragraph` keeps `offset: Offset<f64>`. Glyph positions stay `f32` relative to the paragraph's
origin, the shaper's widening point (ADR-0098 §2, ADR-0092 §4), and the engine composes that
origin in `f64` before narrowing (`text.rs:67-83`). Image source rectangles stay texel coordinates
in `f64`; stroke widths, dash intervals and shader coordinates stay in `Paint`, in the op's local
space.

§3. **The op has an asserted size; fat payloads are boxed; `Path` is one pointer.**

```rust
pub enum DrawOp {
    // …
    DRRect(Box<DRRectOp>),                  // 200 bytes inline before
    ImageRegion(Box<ImageRegionOp>),        // 216
    ImageFiltered(Box<ImageFilteredOp>),    // 144
    // …
}
pub struct DRRectOp { pub outer: RRect, pub inner: RRect, pub paint: Arc<Paint> }
// `ImageRegionOp` and `ImageFilteredOp` carry their variants' former fields, `pub` for reading.

pub struct Path(Arc<PathData>);             // geometry, hint, fill type and bounds; mutators
                                            // go through `Arc::make_mut`
const _: () = assert!(size_of::<DrawOp>() <= 112);
const _: () = assert!(size_of::<RecordedOp>() <= 120);
const _: () = assert!(size_of::<Path>() == size_of::<usize>());
```

The largest inline payloads are then `RRect`, `ClipRRect`, `ClipRSuperellipse`, `Vertices` and
`Atlas`, 104 bytes each. A variant that grows past them boxes its payload, and the assertions make
a forgotten box a build failure. `Vertices` and `Atlas` stay inline: boxing them saves nothing
while `RRect` is 104 bytes inline. `Path::shares_commands_with` compares the one pointer.

§4. **Non-finite input is refused at record, by kind, and counted.**

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct NonFiniteOps {
    pub transform: usize,                   // ops recorded under a CTM with a non-finite element
    pub geometry: usize,                    // ops holding a non-finite value
}
impl DisplayList { pub fn non_finite(&self) -> NonFiniteOps; }
impl DrawOp { fn is_finite(&self) -> bool; }    // exhaustive, no wildcard
```

`DrawOp::is_finite` checks every `f64` the op holds (coordinates, radii, angles, elevation,
opacity, source and tile rectangles, an atlas's transforms, vertices and texture coordinates), its
path's points (through a flag `PathData` keeps current in its mutators: its cached bounds cannot
stand in, since kurbo 0.13.1, the locked version, folds a path's segment boxes with `f64::min` and
`max` (its src/bezpath.rs lines 953-964 and src/rect.rs lines 198-205), which drop a NaN operand), and its paint's
values (stroke width, dash intervals and
phase, shader coordinates, radii, angles and stops), the paint's checked once when the canvas
interns it. A refused op is handled by its kind, in a match with no wildcard:

| Op | Under a non-finite CTM or holding a non-finite value |
|---|---|
| a draw (every painting variant, `Color` and `Paint` included) | omitted |
| a clip | recorded as an intersection with an empty rectangle: what it would have clipped draws nothing |
| `SaveLayer` | recorded with `bounds: None`, so the layer opens and its `RestoreLayer` stays paired |
| `Save`, `Restore`, `RestoreLayer` | recorded under the identity: they hold no geometry, and every scope stays balanced |

Each refusal counts in `NonFiniteOps` (saturating), `append` sums the counts, `Canvas::finish` emits
one `tracing::warn!` when a count is not zero, and `Inspect` prints them. The canvas's finiteness
`debug_assert!`s (`crates/flui-painting/src/canvas/transform.rs:12-13`, `:21-22`, `:30`, `:42-43`,
and the NaN half of `drawing.rs:60-64`) are removed, so a release build and the dev profile CI runs
do the same thing, and the refusal is testable. A finite value too large for the engine's `f32`
payload stays the engine's refusal at its narrowing (`shapes.rs:46-58`).

§5. **The reader yields the transform and the op.**

```rust
#[derive(Clone, Copy, Debug)]
pub struct DrawCommand<'a> { pub transform: &'a Matrix4, pub op: &'a DrawOp }

impl DisplayList {
    pub fn iter(&self) -> impl ExactSizeIterator<Item = DrawCommand<'_>> + '_;
    pub fn get(&self, index: usize) -> Option<DrawCommand<'_>>;
}
impl<'a> IntoIterator for &'a DisplayList { type Item = DrawCommand<'a>; /* … */ }
```

The commands under one entry yield the same reference, so the engine installs a CTM once per entry
and tests "same as before" with `std::ptr::eq` before `==` (`layer_dispatcher.rs:148` compares
values per command). The identity yields `&Matrix4::IDENTITY`, a promoted constant, not a
`static`. `DisplayList::commands()`, `AsRef<[DrawCommand]>`, `Index<usize>` and
`DrawCommand::untransformed` are deleted, since a slice of stored commands would expose the private
id. Field access keeps its spelling (`command.transform`, `command.op`), and the snapshot line's
`xf=[…]` suffix (`crates/flui-rendering/src/testing/snapshot.rs:176`) prints the same values.

§6. **`Canvas::draw_picture` is deleted.** Nothing in production calls it. The composer's splice
is `DisplayList::append_isolated` (`paint.rs:1501-1503`), which keeps the clip isolation that
`inline_picture_replay_owns_its_root_clips` pins. A producer that needs a list placed under a
transform (a painter that caches a drawing) brings back a placed append together with that caller;
under §1 it composes the transform with each appended entry, a pass over entries rather than
commands.

§7. **Clearing commits before a destructor runs.** A list's ops reach user code through their
destructors: a paragraph's font blob holds `FontBytes = Arc<dyn AsRef<[u8]> + Send + Sync>`
(`crates/flui-painting/src/glyphs/registry.rs:12`), and its last owner may be a recorded paragraph.
`DisplayList::clear` and `Canvas::reset` take the contents out with `std::mem::take`, reset every
field (and the canvas's transform, save stack and paint pool), and drop what they took last, so a
destructor that panics leaves an empty list with no cached extent and a reset canvas (ADR-0287 §1's
order). Nothing else in the list runs user code: `record` and `append` call no callback,
`Canvas::transform` runs the caller's `Into<Matrix4>` before it changes the canvas
(`transform.rs:55-58`), and `Canvas::finish` emits its `tracing` events, whose subscriber is user
code, after the list is complete.

### Consequences

- Good, because a command is at most 120 bytes instead of 344 to 352: 200,000 operations hold
  24 MB of commands instead of about 70 MB, plus 128 bytes per distinct transform other than the
  identity, which is one per run of a merged picture whose recorder translates its canvas, and none
  for a render object recorded at its origin.
- Good, because a render object's position lives in its run's one entry: a chunk whose render
  object moves keeps its ops, and only the transform composed with them changes.
- Good, because the placement stays `f64` until the engine composes it, so a recording cache keys
  on the device subpixel phase of the composed translation (ADR-0304).
- Good, because one painter's NaN, which an interpolation can produce (ADR-0149 §1), costs that op
  and not the frame.
- Good, because the budget cannot drift again: the assertions fail the build.
- Bad, because a uniform rounded border (`crates/flui-painting/src/decoration.rs:570`), a focus ring
  and Material's switch, checkbox and chip outlines allocate one box per recorded `DRRect`, and an
  image region or a filtered image one each; a retained chunk pays it once per recording.
- Bad, because 120 bytes is not option 4's 72: about 9.6 MB more at 200,000 operations.
- Bad, because the reader is a view: code that took `commands()` as a slice, indexed a list, or used
  `commands().as_ptr()` as identity
  moves to `iter`, `get` and the address of a yielded `op`, and every match on the three boxed
  variants changes, the engine's dispatch included.
- Bad, because `append` adds a base to each appended id: a pass over the appended commands instead
  of one copy.
- Neutral, because a painter that draws in anchor coordinates
  (`crates/flui-objects/src/proxy/skeleton.rs:203`) bakes its position into its ops by choice and
  repaints when it moves, as `PaintAnchor` documents (`paint.rs:224-244`).
- Neutral, because ADR-0098 §2 is unchanged: the narrowing stays in the engine.

### Confirmation

- The compiler: the three `const` assertions of §3; `TransformId` and `RecordedOp` are private to
  `flui_painting::display_list`; `DrawOp::is_finite` and the kind match of §4 have no wildcard, so
  a new variant states its check.
- `recording_contract` (`crates/flui-painting/tests/main.rs:511`), with rows to be added in
  `tests/recording.rs`: `ops_under_one_transform_share_its_entry` (two draws under one CTM yield
  `ptr::eq` transforms, a translation yields another, and the values are the CTMs),
  `appended_commands_keep_their_transforms`, `a_non_finite_op_is_refused_by_kind` (one row per
  variant and one for a non-finite CTM: a draw is absent, a clip is empty, a save-layer is
  unbounded, the markers remain, `bounds()` and `damage_extent()` stay finite, and `non_finite()`
  counts the reason), and `a_destructor_that_panics_during_reset_leaves_the_canvas_empty`.
- In `flui-engine`: `inline_picture_replay_owns_its_root_clips`
  (`crates/flui-engine/src/clip_layer_readback_tests.rs:726`) is rewritten over `append_isolated`
  with its name and assertions; `a_non_finite_op_is_omitted_and_the_frame_draws` (to be added)
  renders a list holding a NaN rectangle beside a finite one and reads the finite one back.
- The paint snapshot suites (`crates/flui-rendering/tests/paint_fragment_snapshot.rs` among them)
  pass without a regenerated snapshot.
- `cargo xtask panic-messages`: the one new `expect` names its invariant.

## Pros and cons of the options

### 1. Keep the shape and assert its size

- Good, because it is the smallest change, and the assertion makes the stated budget true (at most
  224 and 352).
- Bad, because a command stays 344 to 352 bytes, the engine keeps comparing matrices per command,
  and a render object's position stays copied into each of its commands.

### 2. Interned transforms and boxed payloads, the list's space its layer's

- Good, because it has option 3's sizes (112 and 120).
- Bad, because a consumer may take a yielded transform as absolute in the layer, so a list cannot
  be placed anywhere else: a moved render object is recorded again, and a recording cache keyed by
  content misses on any move.

### 3. As 2, the list's space its own (chosen)

- Good, because ops do not depend on their render object's position, so a retained chunk survives
  a move, at option 2's size.
- Good, because the scalar, the narrowing point and the geometry types stay those of ADR-0098.
- Bad, because a reader that wants layer or root coordinates composes the placement it holds with
  each yielded transform; one that forgets draws or measures the list at its origin.

### 4. Local `f32` geometry under interned `f64` transforms

- Good, because `RRect` shrinks to 48 bytes: with `Atlas` and `Vertices` boxed as well, `DrawOp` is
  64 bytes and a command 72, about 4.8 times smaller than today and 14.4 MB at 200,000 operations.
- Good, because it has option 3's behaviour on a move.
- Bad, because the rounding error has no bound the recorder can choose. An `f32` has a 24-bit
  significand: at magnitude 2^k a value moves by up to 2^(k−24). Under a bound of 2^14 (16,384)
  logical pixels that is 2^-10 px, which a device-pixel ratio of 4 makes 1/256 of a device pixel
  (one 8-bit coverage step at an antialiased edge, a sixty-fourth of the quarter-pixel glyph bin,
  `crates/flui-painting/src/display_list/paragraph.rs:233`, `:255`). An ancestor's zoom multiplies
  it further: under a 64-times scale it is a quarter of a device pixel, a whole glyph bin and a
  quarter of a hairline's one-pixel width (ADR-0098 §6). The recorder knows the ratio
  (`PaintContext::dpr`, `paint.rs:437`) but not its ancestors' transforms; the engine composes in
  `f64` first, so what it narrows is a visible coordinate of target size (`shapes.rs:35`, `:69`).
- Bad, because rebasing an op past the bound has to move what lies outside the op in the same
  frame: a paint's shader coordinates (`crates/flui-engine/src/batches/gradients.rs:403-404`), an
  atlas's sprite transforms, a path's `f64` points. Either the paint is interned again with shifted
  coordinates for every rebased cell, or such ops stay under the unrebased entry beside rebased
  neighbours.
- Bad, because a single op wider than the bound (a box 10^6 pixels tall) keeps its far edge at a
  relative error of 2^-24, the same as the engine's own `f32` extent packing (`shapes.rs:60`), so
  rebasing buys nothing there.
- Bad, because it adds a twin of five geometry types (rectangle, rounded rectangle, superellipse,
  point, offset) with sealed constructors and `f64` accessors, and a second narrowing point, which
  supersedes ADR-0098 §2.

## More information

- The sizes are computed from the definitions the Context cites, not measured; §3's assertions pin
  them at the first build.
- `display_list_record` (`crates/flui-painting/benches/display_list_record.rs`) measures recording;
  its `draw_picture_replay` case becomes an `append_isolated` case, and a `DRRect` joins
  `record_mixed_scene` so the box's cost is visible.
- An `f32` form relative to a chunk's origin belongs, if anywhere, in the engine's cached IR of a
  chunk (ADR-0304), where the composed device scale is known. Revisit option 4 for the list itself
  only if a measured scene of 100,000 nodes, with retained chunks in place, holds more than a third
  of its resident bytes in commands.
- `Matrix4` implements `Eq` over `f64`
  (`crates/flui-foundation/src/geometry/matrix4.rs:812-818`), which ADR-0098 §4 forbids. The
  transform table compares only matrices the canvas has already refused to record when
  non-finite, so nothing here relies on that impl; removing it is a separate fix.
