[← Beat-Flutter plan](2026-06-08-beat-flutter-plan.md) · [Roadmap](../ROADMAP.md)

# Flutter Rendering Pipeline — Deep Study (paint · layout · content leaves)

> **Purpose.** Design grounding for the three pre-widget hardening cycles
> (A: pixel path, B: layout core, C: painting prerequisites) identified by the
> 2026-06-10 readiness audit. Three parallel deep-reads of the `.flutter`
> reference produced this; every claim cites `file:line` in
> `.flutter/flutter-master/packages/flutter/lib/src/`. The goal is not parity —
> it is knowing exactly where Flutter's design is convention-enforced so flui
> can make the same contract **compiler-enforced**, and where Flutter's own
> TODOs admit debt flui should not import.

---

## 1. Paint protocol (object.dart PaintingContext) — Cycle A grounding

### 1.1 Flow ground truth

```
flushPaint (object.dart:1293)
  dirty list holds ONLY repaint boundaries; sorted DEEPEST-first (:1318)
  → PaintingContext.repaintCompositedChild (:123-186)
    boundary layer created once, reused forever (identical-instance assert :162-167)
    → child._paintWithContext(ctx, Offset.zero) (:180)
      _needsLayout → early return (layout-skipped subtree, :3491)
      _wasRepaintBoundary written UNCONDITIONALLY pre-paint (:3560)
      → paint(context, offset)  ← user code drives everything
        → paintChild (:249-267): boundary? stopRecording+_compositeChild
                                  : inline _paintWithContext on SAME canvas
  ctx.stopRecordingIfNeeded() finalizes trailing PictureLayer (:185)
```

- **PictureLayer birth:** lazily at first `canvas` access (`_startRecording`
  :359-365); appended to the container immediately (picture still null), filled
  at `stopRecordingIfNeeded` (:395-420). Ops-before-child and ops-after-child
  land in two PictureLayers around the child's layer — this is how containers
  interleave own drawing with children.
- **Subtree cache hit:** clean boundary in `_compositeChild` = zero painting,
  one `offset` write + `appendLayer` (:277-291). Property-only changes ride
  `markNeedsCompositedLayerUpdate` → `updateLayerProperties` (:199-219).
- **Why deepest-first:** deeper boundaries repaint before ancestors so the
  ancestor's composite step finds them clean and retains wholesale (:275-287).
- **markNeedsPaint** walks to the nearest *stable* boundary
  (`isRepaintBoundary && _wasRepaintBoundary`, :3329-3344).

### 1.2 Invariants Flutter enforces (asserts → our types)

No `markNeedsPaint` during flushPaint (:3322); no paint re-entrancy
(`_debugDoingThisPaint` :3468); paint must not re-dirty itself (:3563); layers
appended only when not recording (:305); appended layer must be orphan
(layer.dart:1218-1235); boundary layer owned by framework, paint must not
replace it (:3147-3155); `updateCompositedLayer` must return the identical
instance (:162-167 — **admitted TODO** issue 102102 at :3108).

### 1.3 Flutter's pain points (do not import)

1. **Dynamic layer typing**: `layer as ClipRectLayer?` casts everywhere
   (proxy_box.dart:1650, 2753, 3001); type frozen at runtime by assert only.
2. **oldLayer manual threading**: every container must pass `oldLayer:`, store
   the return, AND null the field on every non-composited branch
   (proxy_box.dart:1652-1658) — forgetting any of the three silently leaks or
   defeats retained rendering. Convention, not type-checked.
3. **LayerHandle refcounting** (layer.dart:753-816) exists because Dart GC is
   too late for GPU resources; dispose discipline is assert-only.
4. **Canvas aliasing rule is doc-only** (:86-90): caching `context.canvas`
   across a composited child corrupts layer order; nothing prevents it.
5. Layer children rebuilt every frame even on reuse (:512-514) — reuse saves
   engine resources, not tree construction.

### 1.4 Cycle A design (decided by this study)

1. **`paint_raw` bridge, mirroring the proven U19 `perform_layout_raw` shape**
   (render_box.rs:384-431): blanket impl reconstructs typed
   `BoxPaintContext<T::Arity, T::ParentData>` and calls `T::paint`. The render
   object becomes the single paint driver; the pipeline's parallel child
   recursion (owner.rs:2295-2329) and capability interrogation
   (owner.rs:2211-2226) are **deleted**. This restores Flutter's container
   pattern (clip → own ops → paint_child → own ops) which flui currently
   cannot express.
2. **One `paint_child` with Flutter's three-way branch inside**
   (boundary / was-boundary / inline, object.dart:255-266), including the
   pre-paint `WAS_REPAINT_BOUNDARY` write (today written post-children at
   owner.rs:2380-2406 — wrong place).
3. **Compile-time arity beats Flutter:** `Leaf` paint context has no
   `paint_child` method at all — Flutter's runtime "child not visited"
   FlutterError (:3504-3530) becomes a compile error. `Single` takes no index.
4. **Ownership replaces oldLayer threading:** retained-layer cache keyed by
   `RenderId`, owned by the pipeline/context — the push call itself replaces or
   evicts the entry; the "null the field on every branch" footgun is
   structurally impossible. (Today `push_transform` ignores `_old_layer`
   entirely and `CanvasContext` builds a fresh LayerTree per boundary —
   canvas.rs:95-111, 482 — cross-frame retention has no flui equivalent yet;
   Cycle A must leave the keyed-cache seam in the signature.)
5. **Keep the closure-scoped `push_*` API** (canvas.rs:333-373): the borrow
   checker already enforces Flutter's doc-only canvas-aliasing rule. Do NOT
   expose a storable `&mut Canvas`.
6. Keep deliberate divergences: skip-empty-picture (canvas.rs:231; Flutter
   appends empty PictureLayers), mid-paint-mark side queue (owner.rs:2175 —
   implements Flutter's assert-only invariant race-free).

Open for the architect: (a) erased paint ctx — GAT like `LayoutCtxErased` or
concrete `&mut CanvasContext` + child-callback table (paint returns no
geometry, so likely the latter suffices); (b) where the `RenderId → LayerId`
retained cache lives; (c) `needs_compositing` is currently hardcoded `true` at
push sites (owner.rs:2345) despite the U34 bits walk existing — wire it.

---

## 2. Layout / lifecycle / ParentData (object.dart, box.dart) — Cycle B grounding

### 2.1 The relayout-boundary decision (the 4 conditions)

```dart
_isRelayoutBoundary = !parentUsesSize || sizedByParent
                      || constraints.isTight || parent == null;   // object.dart:2845
```

`parentUsesSize` gates ONLY dirty-propagation topology (+ a debug size-access
right), never the layout work itself. `markNeedsLayout` has no explicit walk —
the parent recursion IS the walk, stopping at the first self-declared boundary
(:2658-2700). Flag is `bool?`; `dropChild` resets it to `null` = "unknown"
(:2196-2198). Our port: formula correct (storage/state/geometry.rs:108) but the
bootstrap hardcodes `parent_uses_size = true` (box_protocol.rs:122) so every
node is a non-boundary and all dirt walks to root — correctness-safe,
performance-wrong; real plumb-through needs the flag on `ctx.layout_child`.

### 2.2 THE ParentData answer (kills our owner.rs:1782 hardcode)

**Flutter's pipeline never touches ParentData.** `flushLayout`/`layout`/
`_layoutWithoutResize` have zero knowledge of its type. Type knowledge lives in
exactly two places: the parent's `setupParentData` override (creation at
`adoptChild`, object.dart:2175) and the parent's own
performLayout/paint/hitTest bodies (dynamic casts, flex.dart:815). Storage is
an **erased field on the child** (`ParentData? parentData`, :2085).

Translation for flui (the Cycle B fix shape):

1. Object-safe creation hook on `RenderObject<P>`:
   `fn create_child_parent_data(&self) -> Box<dyn ParentData>` (our
   `ParentData: DowncastSync` base at parent_data/base.rs:48 already supports
   it), invoked at insert/adopt.
2. **Erased `Box<dyn ParentData>` stored per child on `RenderEntry`/
   `RenderState`** — replacing the per-walk `Vec<ChildState<BoxParentData>>`
   that is rebuilt and discarded inside `layout_subtree_borrowed`
   (owner.rs:1735). That vec is the actual reason the pipeline names a concrete
   type today.
3. The downcast to `T::ParentData` happens once per parent per layout, inside
   the blanket bridge where `T` is known (`BoxLayoutCtx::from_erased`,
   render_box.rs:419) — pipeline carries only `&mut dyn ParentData` slots.

Net: we keep the GAT-typed access Flutter can't have (parent's reads are
compile-checked) and gain Flutter's pipeline-neutrality. Also resolves the
`RenderFlex` parallel offset store (`child_offsets: Vec<Offset>` duplicating
what belongs in child parent-data).

### 2.3 Lifecycle contract (we have none of it)

`adoptChild`: setupParentData BEFORE parent pointer; attach AFTER; redepth last
(:2164-2184). `dropChild`: parentData.detach → null → unparent → child.detach;
boundary flag reset to unknown (:2192-2208). `attach(owner)`: **re-enqueues
deferred dirt** — `_needsLayout && boundary-known` → re-run markNeedsLayout
into the NEW owner's queue; same for paint/compositing/semantics (:2477-2503).
Missing attach-re-enqueue is exactly the bug class of the two PR #167 flui-app
wake/render bugs. `dispose`: the ONE mandatory cleanup is the layer handle
(:2068); for Rust, `Drop` subsumes dispose — keep explicit `detach` only for
owner-queue hygiene. Depth is monotone (`redepthChild` :2129-2150) and feeds
the dirty-queue sort; reparent must recompute it.

### 2.4 RenderBox caching architecture (perf prerequisite for text)

Four lazy caches per box (box.dart:1134-1157): intrinsics (keyed
dimension+extent), dry-layout (keyed BoxConstraints), two baseline maps.
Single invalidation point — the `markNeedsLayout` override (box.dart:2840):

```dart
if (_layoutCacheStorage.clear() && parent != null) {
  markParentNeedsLayout();   // escalate ACROSS relayout boundaries!
  return;
}
```

Non-empty cache ⇒ a parent queried intrinsics ⇒ dirt must escalate even past a
relayout boundary — the hidden coupling channel the boundary flag doesn't
model. **Rust-better:** record "parent queried my intrinsics" as an explicit
dependency edge in the slab instead of inferring from cache occupancy. Note
for the port: Dart keys by `double` equality; `f32` keys need bit-pattern or
ordered-float discipline. Reentrancy guards (`_computingThisDryLayout`) become
typestate.

### 2.5 RenderFlex ground truth — audit verdict REVISED

Quoted source (flex.dart): spacing seeds the accumulated size once (`:1229`),
flex space already excludes it (`:1258`), positioning re-adds per gap with
between-space formulas that INCLUDE spacing (`:236-263`), advance at `:1390`.

| Earlier audit claim | Verdict vs source |
|---|---|
| spacing double-count (flex.rs:263/326) | **REFUTED** — two independent accumulators, algebraically consistent with Flutter for all six alignments |
| `free_space` not clamped (flex.rs:341) | **CONFIRMED** — Flutter clamps `max(0.0, …)` at `:1339`; overflow leaks negative space into End/Center/Space* |
| Stretch never tightens constraints | **CONFIRMED** — Flutter: `tightFor(cross = max)` for non-flex (`:889-898`), `min = max` for flex (`:923,927`) |
| unbounded main + flex → zero-size children | **CONFIRMED** — Flutter demotes flex children to inflexible when `!canFlex` (`:1232`) |
| (new) `MainAxisSize` missing entirely | flui always shrink-wraps (flex.rs:330); Flutter default `MainAxisSize.max` fills `maxMainSize` (`:1298`) — alignment is dead under loose constraints |
| (new) non-flex cross constraints propagate `min` | Flutter passes LOOSE cross unless stretch (`:893`) |

Also absent: baseline alignment path (`_AscentDescent` fold, `:1242-1296`),
intrinsics (`maxFlexFraction * totalFlex + inflexibleSpace`, `:716-733`), and
the dry/real algorithm sharing (`_computeSizes` parameterized by
`ChildLayouter`, `:1095/1329` — the pattern flui should port wholesale).

### 2.6 invokeLayoutCallback (LayoutBuilder) — cannot be cargo-culted

Flutter permits mid-layout tree mutation via
`invokeLayoutCallback` + `_shouldMergeDirtyNodes` re-sort dance
(object.dart:1088-1102, 3017-3029). Borrow rules forbid the aliasing version
outright; flui needs an explicit design (queued mutations applied between
boundary iterations). Deferred design item — blocks `LayoutBuilder`, not the
first widget wave.

---

## 3. Content-leaf contracts (paragraph/image/decorated_box) — Cycle C grounding

### 3.1 The two hooks that block ALL three leaves

1. **`dispose`**: TextPainter disposal (paragraph.dart:562-568), `ui.Image`
   handle release (image.dart:447-451), BoxPainter disposal
   (proxy_box.dart:2461). flui has no hook → in Rust, `Drop` + explicit
   `detach` for queue hygiene.
2. **`mark_needs_paint` from OUTSIDE pipeline phases** (async wake): image
   frames arrive on the event loop (image.dart:97), decoration images repaint
   via the BoxPainter `onChanged` callback (proxy_box.dart:2474,
   decoration_image.dart:413-424), opacity animations tick (image.dart:179).
   flui's substrate exists (`add_node_needing_paint`), missing is the
   object-held wake handle. **Rust-better:** typed `RepaintHandle` (channel to
   the scheduler per ADR-0002 control plane); subscriptions as RAII guards —
   flui-animation's `ListenerSubscription` is the prior art. Kills Flutter's
   detach/recreate-painter hack ("GIFs stop animating on GlobalKey reparent",
   proxy_box.dart:2448-2459) and listener identity juggling
   (widgets/image.dart:1165/1282/1350).

### 3.2 RenderParagraph essentials

- Setter→dirty table is large (paragraph.dart:418-758); the key subtlety:
  `textAlign` is paint-only at render level but layout-invalidating at painter
  level, so `paint()` and every geometry query defensively re-layout
  (:1007-1011). **Do not copy** — see 3.4.
- Intrinsics run on a **clone painter** (`_textIntrinsicsCache`, :389-409)
  because computing them destroys painter state — admitted wart.
- Baseline comes from the shaper (`paragraph.alphabeticBaseline`,
  text_painter.dart:342-347). **No hardcoded ratios anywhere** — flui's 80/20
  has no Flutter counterpart and must die.
- performLayout: layout inline children → set placeholder dims → layout text →
  `size = constraints.constrain(textSize)` → overflow detect
  (`didExceedMaxLines`!) → ellipsis handled by the painter's `ellipsis` string
  AT LAYOUT TIME; fade builds a gradient shader (:933-998).
- Hit testing: closest glyph → grapheme bounds contain? → span's recognizer as
  HitTestTarget, else inline children (:823-848).

### 3.3 RenderImage essentials

Sizing is ~10 portable lines: `tightFor(w,h).enforce(constraints)`, null image
→ smallest, else `constrainSizeAndAttemptToPreserveAspectRatio(dims/scale)`
(image.dart:349-361). Setter discipline: `image=` disposes old handle, clone
check, `markNeedsPaint` always + `markNeedsLayout` only if intrinsic-driven
(:84-101). The stream machinery stays widget-side; the render layer needs only
the §3.1 hooks + owned image slot. **Rust-better:** `Arc<Image>` identity =
`Arc::ptr_eq` (Flutter needs `isCloneOf` runtime checks); frames via watch
channel; GPU upload sits data-plane-side per ADR-0002.

### 3.4 TextPainter — the structural Rust win

Flutter cannot update paint attributes without recreating the engine paragraph
("no API to only make those updates", text_painter.dart:1335-1352): a color
change re-shapes at paint time; alignment is baked into layout. **flui design:
split the shaped layout (glyph runs, line breaks — expensive) from paint
attributes (color/decoration — cheap)**: paint-attr change re-emits draw
commands over the same shaped lines with zero reshape; alignment is a paint
offset computed at draw; intrinsics are a pure function over the shaper
(cosmic-text exposes line extents) — no clone painter. Typed
`Invalidation {None, Semantics, Paint, Layout}` computed by prop-diff replaces
Flutter's hand-maintained per-setter convention.

### 3.5 RenderDecoratedBox essentials

BoxPainter lifecycle: lazy create at first paint with `markNeedsPaint` AS the
callback (proxy_box.dart:2474); callback exists because DecorationImage
resolves async inside paint (box_decoration.dart:535-540); decoration setter
disposes+nulls painter; detach disposes + marks paint to force re-subscribe.
Paint order: background → child → foreground (:2472-2519). Hit test delegates
to decoration geometry (`decoration.hitTest`, :2467). All flui canvas
primitives needed already exist (rect/rrect/gradient/shadow/drrect/image);
only the orchestrating painter is missing.

---

## 4. Consolidated "we do it better" register (compiler-enforced vs convention)

| Flutter convention/assert | flui mechanism |
|---|---|
| ParentData dynamic casts fail at use-site in release | GAT-typed parent access in `BoxLayoutCtx`; one checked downcast at the blanket bridge |
| mutation-during-layout debug asserts (`_debugCanPerformMutations` web) | typestate pipeline phases + `SubtreeBorrows` disjoint `&mut` (done) |
| "child not visited by visitChildren" runtime FlutterError | arity-typed paint/layout contexts: Leaf has no `paint_child` — compile error |
| oldLayer threading + null-on-every-branch discipline | pipeline-owned retained-layer cache keyed by RenderId; push replaces/evicts |
| LayerHandle refcount + assert-guarded dispose | slab arena + `Drop` (RAII) |
| canvas-aliasing rule in prose | closure-scoped `push_*` — borrow checker enforces |
| paint-attr change ⇒ paragraph re-shape | shaped-layout/paint-attr split (3.4) |
| listener identity juggling for image streams | watch channels + RAII subscription guards |
| intrinsic-cache occupancy as hidden dependency signal | explicit "intrinsics queried" dependency edge in slab |
| exception swallowing in layout (`_reportException`) | typed `RenderError` + Poisoned + keep-dirty-retry (done) |

---

## 5. Revised cycle plans (supersedes the readiness-audit sketch)

**Cycle A — pixel path** (unchanged scope, now with decided design §1.4):
paint_raw bridge (U19 shape) → single paint_child with three-way branch →
delete pipeline parallel recursion → wire needs_compositing bit → damage-gate +
visual-update wiring (flui-app) → integration test ColoredBox→DrawRect→Layer.

**Cycle B — layout core** (revised by §2):
1. Erased `Box<dyn ParentData>` on child + `create_child_parent_data` hook +
   bridge-side downcast (kills owner.rs:1782).
2. Lifecycle: adopt/drop semantics on insert/remove, attach-re-enqueue rule,
   depth recompute on reparent; `Drop`-based dispose.
3. Flex: clamp free_space, Stretch tightening, unbounded-main demote,
   `MainAxisSize`, loose cross for non-flex — **not** the refuted
   spacing "fix"; port the `ChildLayouter`-parameterized dry/real sharing.
4. `parent_uses_size` plumb-through on `layout_child` (boundary topology).
5. RenderBox caches (intrinsics/dry/baseline) + explicit dependency edge.

**Cycle C — painting prerequisites** (ordered by §3.1 blocking):
1. dispose/`Drop` + RepaintHandle async wake (unblocks all leaves).
2. TextPainter: shaped/paint split (3.4), real intrinsics, maxLines+ellipsis
   enforcement, span-preserving rich shaping (`set_rich_text`), named font
   families, shaper-derived baselines (kill 80/20).
3. BoxPainter trait + BoxDecoration painter (owned lifecycle).
4. RenderImage slot + sizing port + texture-upload seam.

Open design items deferred past A/B/C: invokeLayoutCallback analog (§2.6),
cross-frame retained layers (§1.4-4 seam only), semantics assembly for
paragraph (§3.2 last).
