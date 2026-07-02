# `RenderSliverPersistentHeader` family — plan (oracle-verified)

Core.2 catalog item (next of ≈10 remaining after `RenderAnimatedSize` closed same-day, per `docs/research/widget-renderobject-map.md`). Oracle: `.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_persistent_header.dart` (837 lines, read in full) plus its widget-layer counterpart `.flutter/flutter-master/packages/flutter/lib/src/widgets/sliver_persistent_header.dart` (516 lines, read in full) and the three newer sibling widgets `pinned_header_sliver.dart`, `sliver_floating_header.dart`, `sliver_resizing_header.dart`.

## Headline verdict, up front

1. **No new ADR is needed.** ADR-0013 (`docs/adr/ADR-0013-render-object-attach-self-dirty-handle.md`) already shipped, and — critically — its `attach`/`detach` lifecycle pair is **already present on the `RenderSliver` trait itself**, not just `RenderBox`: `crates/flui-rendering/src/traits/render_sliver.rs:463-478` (defaulted no-op) and forwarded through the blanket `impl RenderObject<SliverProtocol>` at `:610-616`. `RenderSliverFloatingPersistentHeader`'s self-driven snap-animation controller is architecturally identical to `RenderAnimatedSize`'s already-closed case (inject an `AnimationController` at construction per D2, subscribe in `attach` → `handle.mark_needs_layout()`, unsubscribe in `detach`). This is pure application of already-shipped infrastructure — a DEV task, not an ARCH-GATE task.
2. **No new `flui-rendering` delegate trait is needed either.** Flutter's `SliverPersistentHeaderDelegate` (the `build`/`shouldRebuild` object) is a **widget-layer** concept living in `widgets/sliver_persistent_header.dart`, not in the render objects this task scopes. The base `rendering/sliver_persistent_header.dart` classes take `minExtent`/`maxExtent` as **abstract getters implemented by the concrete render-object subclass** (`:136,144`) — never delegate-sourced at the render layer. §4 below has the full argument, including a leapfrog recommendation informed by Flutter's own newer sibling widgets.
3. This is **purely sliver-family-local work** reusing FLUI's existing `SliverConstraints`/`SliverGeometry`/`SliverLayoutContext`/`SliverPhysicalParentData` types, following the exact "Single-arity Sliver wrapping one Box child" shape already established by `RenderSliverToBoxAdapter` and the `RenderSliverFillRemaining*` family in `crates/flui-objects/src/sliver/`.

## 1. Oracle — `rendering/sliver_persistent_header.dart` (line-cited)

**Base class `RenderSliverPersistentHeader`** (`:120-345`, abstract, `Single` box child):
- Fields: `stretch_configuration: Option<OverScrollHeaderStretchConfiguration>` (`:126,178`), `_last_stretch_offset` (`:130`), `_needs_update_child = true` (`:159`), `_last_shrink_offset = 0.0` (`:163`), `_last_overlaps_content = false` (`:167`).
- Abstract: `max_extent`/`min_extent` getters (`:136,144`) — "should not be based on the child... if it changes, call `markNeedsLayout`."
- `child_extent` (`:147-157`): projects the child's box size onto `constraints.axis()`.
- `update_child(shrink_offset, overlaps_content)` (`:201`): **empty default body** — this is the hook subclasses override to react to shrink/overlap changes; the base class itself does nothing.
- `mark_needs_layout` override (`:203-209`): sets `_needs_update_child = true` before calling super — "automatically called whenever the child's intrinsic dimensions change."
- `layout_child(scroll_offset, max_extent, overlaps_content=false)` (`:220-262`), called by every concrete subclass's `perform_layout`:
  - `shrink_offset = min(scroll_offset, max_extent)`.
  - Calls `update_child(shrink_offset, overlaps_content)` **only if** `_needs_update_child || _last_shrink_offset != shrink_offset || _last_overlaps_content != overlaps_content` (`:222-231`) — a change-detection guard, not an unconditional call every layout.
  - Debug assert `min_extent <= max_extent` (`:233-242`).
  - Stretch: `stretch_offset = stretch_configuration.is_some() && constraints.scroll_offset == 0.0 ? constraints.overlap.abs() : 0.0` (`:243-246`).
  - Lays out child: `constraints.as_box_constraints(0.0, max(min_extent, max_extent - shrink_offset) + stretch_offset)` (`:248-253`).
  - Fires `stretch_configuration.on_stretch_trigger` **exactly once per crossing** (`:255-260`): `stretch_offset >= trigger_offset && _last_stretch_offset <= trigger_offset` — an edge-trigger, not a level-trigger; must not be conflated into "fire every frame stretch_offset exceeds trigger."
- `paint` (`:311-331`): offset computed from `applyGrowthDirectionToAxisDirection(axis_direction, growth_direction)`: `Up`/`Left` → `paint_extent - child_main_axis_position - child_extent`; `Right`/`Down` → `child_main_axis_position`. **FLUI trap-avoidance**: FLUI's established sliver-adapter convention (`RenderSliverToBoxAdapter`, `RenderSliverFillRemaining*`) computes this offset **once in `perform_layout` via `ctx.position_child`**, not in a custom `paint` override. The existing shared helper `flui_rendering::constraints::child_paint_offset(constraints, geometry, layout_offset, child_main_extent)` (`crates/flui-rendering/src/constraints/sliver_layout.rs:10-30`) already implements exactly this `right_way_up`-gated formula — it just parameterizes by `layout_offset` (assumed `= scroll_offset`-relative) rather than by an arbitrary `child_main_axis_position`. **The correct reuse is: call the existing helper with `layout_offset := child_main_axis_position + constraints.scroll_offset`**, since the helper internally computes `child_main_axis_position = layout_offset - constraints.scroll_offset`, which cancels back to exactly `child_main_axis_position`. This is the same trick `RenderSliverFillRemaining`'s local wrapper already uses implicitly (its `child_main_axis_position` is always `-scroll_offset` → `layout_offset` param passed as `0.0`). No new paint-offset helper needs to be written.
- `hit_test_children`/`applyPaintTransform`/`describeSemanticsConfiguration` (`:284-337`): straightforward `RenderSliverHelpers` forwards — direct analogs of `RenderSliverToBoxAdapter`'s existing `hit_test`/`child_main_axis_position` overrides.

**`RenderSliverScrollingPersistentHeader`** (`:352-397`) — "no effort to avoid overlapping":
- `update_geometry()` (`:365-383`): `stretch_offset = stretch_configuration.is_some() ? overlap.abs() : 0.0`; `paint_extent = max_extent - scroll_offset`; `cache_extent = calculate_cache_offset(0.0, max_extent)`; `geometry = SliverGeometry{ cache_extent, scroll_extent: max_extent, paint_origin: min(overlap, 0.0), paint_extent: clamp(paint_extent, 0.0, remaining_paint_extent), max_paint_extent: max_extent + stretch_offset, has_visual_overflow: true }`; **returns** `stretch_offset > 0 ? 0.0 : min(0.0, paint_extent - child_extent)` — this return value becomes `child_main_axis_position`.
- `perform_layout` (`:385-389`): `layout_child(constraints.scroll_offset, max_extent)` (note: **no** `overlaps_content` arg — defaults `false`), then `_child_position = update_geometry()`.
- `child_main_axis_position` (`:391-396`): returns the cached `_child_position`.

**`RenderSliverPinnedPersistentHeader`** (`:404-473`) — "never scrolls off... avoids overlapping":
- `perform_layout` (`:419-445`): `overlaps_content = constraints.overlap > 0.0`; `layout_child(scroll_offset, max_extent, overlaps_content)`; `effective_remaining_paint_extent = max(0, remaining_paint_extent - overlap)`; `layout_extent = clamp(max_extent - scroll_offset, 0.0, effective_remaining_paint_extent)`; `stretch_offset = stretch_configuration.is_some() ? overlap.abs() : 0.0`; geometry: `{scroll_extent: max_extent, paint_origin: overlap, paint_extent: min(child_extent, effective_remaining_paint_extent), layout_extent, max_paint_extent: max_extent + stretch_offset, max_scroll_obstruction_extent: min_extent, cache_extent: layout_extent > 0.0 ? -cache_origin + layout_extent : layout_extent, has_visual_overflow: true}`.
- `child_main_axis_position` (`:448`): **always `0.0`** — the defining "pinned" behavior. **Note `max_scroll_obstruction_extent: min_extent`** — this is exactly the value that flows into `SliverConstraints.overlap` for subsequent siblings; already confirmed wired end-to-end in FLUI's `RenderViewport` (`crates/flui-objects/src/sliver/viewport.rs:350,373`: `overlap = max_paint_offset - layout_offset` where `max_paint_offset` accumulates each sibling's committed `max_scroll_obstruction_extent`). No viewport change is needed — the stacking interaction Just Works once this sliver reports the field.
- `show_on_screen` override (`:450-472`): trims the target rect to `[0, child_extent]` in the growth-adjusted axis before delegating to `super.showOnScreen`.

**`RenderSliverFloatingPersistentHeader`** (`:508-787`) — the trickiest part:
- Fields: `_controller: Option<AnimationController>`, `_animation`, `_last_actual_scroll_offset: Option<f32>`, `_effective_scroll_offset: Option<f32>`, `_last_started_scroll_direction: Option<ScrollDirection>` (pointer-scroll bookkeeping, `:520-527`), `_child_position: Option<f32>`, `vsync`, `snap_configuration`, `show_on_screen_configuration`.
- `detach` (`:534-538`): `controller.dispose(); controller = None` — **eager dispose**, matching the AnimatedSize plan's already-documented FLUI divergence (§ below).
- `update_geometry()` (`:580-597`): `stretch_offset` as before; `paint_extent = max_extent - effective_scroll_offset` (uses **`_effective_scroll_offset`**, not raw `constraints.scroll_offset` — this is the key divergence from Scrolling's `update_geometry`); `layout_extent = max_extent - constraints.scroll_offset` (this one **does** use raw scroll offset); geometry assembled analogously; returns `stretch_offset > 0 ? 0.0 : min(0.0, paint_extent - child_extent)`.
- `_update_animation(duration, end_value, curve)` (`:599-614`): lazily builds `_controller` (`AnimationController(vsync, duration)` with a listener: `if effective_scroll_offset != animation.value { effective_scroll_offset = animation.value; markNeedsLayout(); }`); rebuilds `_animation = controller.drive(Tween(begin: effective_scroll_offset, end: end_value).chain(CurveTween(curve)))`.
- `update_scroll_start_direction` (`:616-620`), `maybe_start_snap_animation` (`:622-641`), `maybe_stop_snap_animation` (`:643-647`): **public API, never called from inside this file** — driven externally by `_FloatingHeaderState`/`_isScrollingListener` in the widget layer (`widgets/sliver_persistent_header.dart:202-244`), which listens to `ScrollPosition.isScrollingNotifier` and calls these on scroll start/stop. **This confirms §3's scope boundary below.**
- **`perform_layout` — the re-reveal state machine** (`:649-689`), the single highest-risk formula in this file:
  ```
  if _last_actual_scroll_offset.is_some()
     && (constraints.scroll_offset < _last_actual_scroll_offset
         || _effective_scroll_offset < max_extent)
  {
      let mut delta = _last_actual_scroll_offset - constraints.scroll_offset;
      let allow_floating_expansion =
          constraints.user_scroll_direction == ScrollDirection::Forward
          || _last_started_scroll_direction == Some(ScrollDirection::Forward);
      if allow_floating_expansion {
          if _effective_scroll_offset > max_extent { _effective_scroll_offset = max_extent; }
      } else if delta > 0.0 {
          delta = 0.0;  // disallow expansion; shrinking (delta<0) still allowed
      }
      _effective_scroll_offset = clamp(_effective_scroll_offset - delta, 0.0, constraints.scroll_offset);
  } else {
      _effective_scroll_offset = constraints.scroll_offset;
  }
  let overlaps_content = _effective_scroll_offset < constraints.scroll_offset;
  layout_child(_effective_scroll_offset, max_extent, overlaps_content);
  _child_position = update_geometry();
  _last_actual_scroll_offset = Some(constraints.scroll_offset);
  ```
  **Read this precisely**: the outer `if` gate is "have we laid out before, AND (we're scrolling backward OR we're already partially revealed)" — i.e. the re-reveal machinery only engages once a prior frame exists; the very first layout always takes the `else` branch (`effective_scroll_offset = scroll_offset`, no float lag).
- `show_on_screen` (`:691-774`): computes a target extent/rect in **child coordinate space** (not sliver), clamped by `show_on_screen_configuration`, and — if expansion is needed — drives an animation via `_update_animation` + `controller.forward(from: 0.0)`.
- `child_main_axis_position` (`:776-780`): `_child_position ?? 0.0`.

**`RenderSliverFloatingPinnedPersistentHeader`** (`:797-836`) — extends Floating, overrides **only** `update_geometry`:
```
let min_allowed_extent = if remaining_paint_extent > min_extent { min_extent } else { remaining_paint_extent };
let paint_extent = max_extent - effective_scroll_offset;
let clamped_paint_extent = clamp(paint_extent, min_allowed_extent, remaining_paint_extent);
let layout_extent = max_extent - constraints.scroll_offset;
geometry = SliverGeometry{
    scroll_extent: max_extent, paint_origin: min(overlap, 0.0),
    paint_extent: clamped_paint_extent,
    layout_extent: clamp(layout_extent, 0.0, clamped_paint_extent),
    max_paint_extent: max_extent + stretch_offset,
    max_scroll_obstruction_extent: min_extent,  // ← the "pinned" contribution to sibling overlap
    has_visual_overflow: true,
};
return 0.0;  // ← always pinned at the leading edge, never a nonzero child_position
```
Everything else — the entire `perform_layout` re-reveal state machine, `_update_animation`, `show_on_screen`, `attach`/`detach` — is **verbatim identical** to `RenderSliverFloatingPersistentHeader`. This is the single most important structural fact for the Rust design (§5).

## 2. Widget-layer scope determination (why no render-layer delegate)

Read `widgets/sliver_persistent_header.dart` in full. The `SliverPersistentHeaderDelegate` abstract class (`:20-113`, `build`/`minExtent`/`maxExtent`/`vsync`/`snapConfiguration`/`stretchConfiguration`/`showOnScreenConfiguration`/`shouldRebuild`) is consumed by a **custom `RenderObjectElement`**, `_SliverPersistentHeaderElement` (`:250-347`), which:
- Overrides `update_child` on a private mixin `_RenderSliverPersistentHeaderForWidgetsMixin` (`:369-390`) to call `_element._build(shrink_offset, overlaps_content)` (`:381-384`), which does `owner.buildScope(this, () { child = update_child(child, delegate.build(this, shrink_offset, overlaps_content), null); })` (`:297-316`) — **a synchronous Element rebuild triggered from inside the render object's own `layoutChild` call**, i.e. the render layer calling back into the Element/build layer mid-layout.
- `minExtent`/`maxExtent` on this mixin are **forwarded reads of `delegate.minExtent`/`delegate.maxExtent`** (`:372-378`) — the base `rendering/` classes never see a delegate; only this widget-layer mixin does.

**This "build during layout" machinery does not exist anywhere in FLUI today** (no `LayoutBuilder` analog, no render-object-holds-an-Element-reference precedent — confirmed absent by grep across `flui-widgets`/`flui-view`/`flui-rendering`). Introducing it would be a genuine new architectural capability, not sliver-family-local work.

**Confirmation this is avoidable, not just deferred-with-a-gap**: Flutter's own newer sibling widgets bypass this entirely. `PinnedHeaderSliver` (`widgets/pinned_header_sliver.dart:84-135`) and `SliverFloatingHeader` (`widgets/sliver_floating_header.dart:174-352`) subclass `RenderSliverSingleBoxAdapter` **directly** — not `RenderSliverPersistentHeader` — take an ordinary `Widget child` reconciled through the normal Element tree (no delegate, no rebuild-in-layout, no `minExtent`/`maxExtent` concept at all — the header's extent is just the child's natural box size), and reimplement the pinned/floating layout math inline. `SliverResizingHeader` (`widgets/sliver_resizing_header.dart:133: class _RenderSliverResizingHeader extends RenderSliver`) does the same. Flutter shipped these specifically because the delegate+rebuild-in-layout API is heavier than most call sites need.

**Verdict**: the render objects this task scopes (`rendering/sliver_persistent_header.dart`'s abstract base + 4 concrete classes) are entirely usable with an **ordinary, already-existing single Box child** — `update_child(shrink_offset, overlaps_content)` stays the oracle's own no-op-by-default hook (`:201`), and `min_extent`/`max_extent` are plain fields/constructor args on the concrete FLUI struct, exactly like `RenderConstrainedBox`'s numeric config, **not** a delegate object. This closes point 4 of the task: **no `SliverPersistentHeaderDelegate` Rust trait is needed in `flui-rendering` for this render-object family**, and none should be added under `experimental-delegates` gating either — the existing gated/ungated delegates (`SliverGridDelegate`, `FlowDelegate`, `SingleChildLayoutDelegate`, `MultiChildLayoutDelegate`, all now ungated per `crates/flui-rendering/AGENTS.md:22`) are all **pure-geometry-or-paint** delegates with no widget-building responsibility — a `SliverPersistentHeaderDelegate` would be categorically different (View-producing) and doesn't belong in this crate at all. When FLUI eventually ports the widget, model it on `PinnedHeaderSliver`/`SliverFloatingHeader` (ordinary `Child`, normal reconciliation) rather than the original delegate+rebuild design — this is the AGENTS.md rule-2 "leapfrog" call, and it is the *lower-effort* path, not just the more Rust-idiomatic one.

## 3. `TickerProvider`/vsync scope

**In scope for the render object** (already-solved infra, D2-style injection): `RenderSliverFloatingPersistentHeader` holds an **injected** `AnimationController` (built by whatever eventually constructs the render object — a future widget's `State`, exactly like `RenderAnimatedSize`), subscribes to it in `attach(handle)` with `move || handle.mark_needs_layout()`, unsubscribes in `detach()`. `crates/flui-animation` is already a normal (non-dev) dependency of `flui-objects` (`crates/flui-objects/Cargo.toml:17` — promoted for `RenderAnimatedSize`; no manifest change needed here).

**Out of scope for this render-object pass** (confirmed by reading the oracle's own call sites): `update_scroll_start_direction`/`maybe_start_snap_animation`/`maybe_stop_snap_animation` are **public methods with zero internal callers** in `rendering/sliver_persistent_header.dart` — they exist purely so a higher layer can drive them. In Flutter that higher layer is `_FloatingHeaderState`/`_isScrollingListener` (`widgets/sliver_persistent_header.dart:202-244`), which listens to `ScrollPosition.isScrollingNotifier` (a `Scrollable`-level notifier) and locates the render object via `findAncestorRenderObjectOfType`. **This is `SliverAppBar`/`Scrollable`-widget-layer wiring** — it requires FLUI's own scroll-gesture/`ScrollPosition` notification plumbing (does that even exist yet? — not investigated here, explicitly out of scope) and is correctly deferred to the eventual `SliverAppBar` widget pass named in the task. The render object should expose these three methods verbatim (oracle parity, harness-testable in isolation by calling them directly — §6) without wiring a caller.

**Also out of scope, confirmed absent infrastructure-wide**: `RenderObject::show_on_screen` does not exist anywhere in `flui-rendering` (grepped, zero hits) — so `RenderSliverPinnedPersistentHeader::show_on_screen`/`RenderSliverFloatingPersistentHeader::show_on_screen` overrides have no base method to override. Document as deferred (matches the catalog's existing precedent of documenting deferred edges rather than faking them, e.g. `RenderCustomPaint`'s deferred repaint-listenable).

## 4. FLUI struct/trait shape

**Precedent to follow**: `crates/flui-objects/src/proxy/clip.rs:1-51` documents FLUI's established idiom for "Dart's diamond-shaped private-mixin family" — collapse to **one generic struct + one sealed trait**, monomorphized per variant via type aliases (`type RenderClipRect = RenderClip<Rect<Pixels>>`, etc.), avoiding `Box<dyn>`/vtable dispatch in the hot path.

**Why persistent headers need *two* such generics, not one**: unlike the Clip family (all 4 variants share an *identical* field set, differing only in per-shape methods), the header family has a genuine field-set split — `Scrolling`/`Pinned` carry no animation state at all, while `Floating`/`FloatingPinned` carry a substantial extra cluster (`controller`, `animation`, `snap_configuration`, `_last_actual_scroll_offset`, `_effective_scroll_offset`, `_last_started_scroll_direction`). Forcing all 4 into one generic struct would leave `Scrolling`/`Pinned` with dead `Option`-al animation fields — worse than the Clip precedent, not equivalent to it. The oracle's own hierarchy already reflects this split (`RenderSliverFloatingPinnedPersistentHeader extends RenderSliverFloatingPersistentHeader`, not the base class directly).

Proposed shape:

```rust
// Shared by ALL FOUR variants — mirrors the base RenderSliverPersistentHeader
// class (:120-345): stretch config, shrink-offset/overlaps-content change
// detection, child_extent, layout_child, paint-offset computation.
struct PersistentHeaderCore {
    stretch_configuration: Option<OverScrollHeaderStretchConfiguration>,
    last_stretch_offset: f32,
    needs_update_child: bool,      // starts true, :159
    last_shrink_offset: f32,
    last_overlaps_content: bool,
    min_extent: f32,               // plain config field — see §2, no delegate
    max_extent: f32,
}
impl PersistentHeaderCore {
    // Mirrors layout_child (:220-262) exactly, including the change-detection
    // guard and the edge-triggered on_stretch_trigger callback.
    fn layout_child(
        &mut self, ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
        constraints: &SliverConstraints, scroll_offset: f32, overlaps_content: bool,
        update_child: impl FnOnce(f32, bool),  // subclass's own update_child hook
    ) -> f32 /* child_extent, post-layout */ { ... }
}

// Sealed trait carrying ONLY the per-variant formula difference for the
// non-animated pair — mirrors ClipGeometry's role.
sealed trait StaticHeaderMode {
    fn update_geometry(core: &PersistentHeaderCore, constraints: &SliverConstraints, child_extent: f32) -> (SliverGeometry, f32 /* child_position */);
    fn overlaps_content(constraints: &SliverConstraints) -> bool;      // Pinned: overlap>0.0; Scrolling: always false
    const CHILD_MAIN_AXIS_POSITION_IS_ALWAYS_ZERO: bool;               // true only for Pinned
}
struct RenderSliverPersistentHeaderStatic<M: StaticHeaderMode> { core: PersistentHeaderCore, child_position: f32, _mode: PhantomData<M> }
struct ScrollingMode; struct PinnedMode { show_on_screen_configuration: ... } // Pinned needs one extra field —
// see note below; a unit-struct mode type can't carry it, so PinnedMode
// is NOT a bare marker — plan this as a real field on the outer struct
// instead (see "Correction" below).
pub type RenderSliverScrollingPersistentHeader = RenderSliverPersistentHeaderStatic<ScrollingMode>;
pub type RenderSliverPinnedPersistentHeader   = RenderSliverPersistentHeaderStatic<PinnedMode>;

// Sealed trait carrying ONLY the update_geometry difference for the
// animated pair (perform_layout's re-reveal state machine is IDENTICAL
// between Floating and FloatingPinned — shared, not duplicated).
sealed trait FloatingHeaderMode {
    fn update_geometry(core: &PersistentHeaderCore, constraints: &SliverConstraints,
                        effective_scroll_offset: f32, child_extent: f32) -> (SliverGeometry, f32);
}
struct RenderSliverFloatingHeaderBase<M: FloatingHeaderMode> {
    core: PersistentHeaderCore,
    controller: Option<AnimationController>,
    animation: Option<CurvedAnimation<ArcCurve>>,
    snap_configuration: Option<FloatingHeaderSnapConfiguration>,
    show_on_screen_configuration: Option<PersistentHeaderShowOnScreenConfiguration>,
    last_actual_scroll_offset: Option<f32>,
    effective_scroll_offset: Option<f32>,
    last_started_scroll_direction: Option<ScrollDirection>,
    child_position: Option<f32>,
    _mode: PhantomData<M>,
}
struct FloatingMode; struct FloatingPinnedMode;
pub type RenderSliverFloatingPersistentHeader       = RenderSliverFloatingHeaderBase<FloatingMode>;
pub type RenderSliverFloatingPinnedPersistentHeader = RenderSliverFloatingHeaderBase<FloatingPinnedMode>;
```

**Correction to flag explicitly (a real design fork, not a nit)**: `PinnedMode` needs its own `show_on_screen_configuration` field (oracle `:417`), which a zero-sized marker-type mode parameter cannot carry. Two honest options: (a) drop the generic-mode idea for the static pair and just write two small, separate, non-generic structs (`RenderSliverScrollingPersistentHeader`, `RenderSliverPinnedPersistentHeader`), each embedding `PersistentHeaderCore` directly and implementing its own `perform_layout`/`update_geometry`/`child_main_axis_position` — given the *entire* divergence between these two is small (one geometry formula, ~15 lines, plus one extra field), a generic is arguably overkill here, unlike the animated pair where the generic saves duplicating the ~40-line re-reveal state machine; or (b) keep the generic and thread `show_on_screen_configuration` through `PersistentHeaderCore` unconditionally (harmless `None` on `Scrolling`, since the field is cheap and both variants are `Single`-arity anyway). **Recommendation: (a) for the static pair, generic-over-`FloatingHeaderMode` for the animated pair only** — apply the `RenderClip<S>`-style generic exactly where it earns its keep (deduplicating the re-reveal state machine, the one place a hand-copy is a real correctness risk) and skip it where the divergence is trivial enough that a generic just adds ceremony. This is a one-paragraph decision for whoever picks up implementation, not a blocker.

All four concrete types: `type Arity = Single; type ParentData = SliverPhysicalParentData` (matching `RenderSliverToBoxAdapter`/`RenderSliverFillRemaining*` exactly, `crates/flui-rendering/src/parent_data/sliver_variants.rs:384-399`).

## 5. Traps a naive port would fall into

1. **Uniform "call `update_child` every layout"** — the oracle's `layout_child` only calls it under the three-way change-detection guard (`:222-224`). A naive port that calls it unconditionally is harmless *today* (since `update_child` defaults to no-op and there's no delegate to rebuild — §2), but becomes a real bug the moment any future subclass overrides `update_child` to do real work (e.g. an internal ticker resize) — implement the guard now so the contract is right from day one.
2. **`update_geometry`'s `paint_extent` vs `layout_extent` asymmetry in the Floating variants** — `paint_extent` uses `_effective_scroll_offset` but `layout_extent` uses raw `constraints.scroll_offset` (`:586-587`). Copying one formula for both (an easy skim-read mistake) breaks the visual "float back into view without pushing siblings" effect, which is the entire point of the floating variant.
3. **The re-reveal state machine's outer gate is a *conjunction with history*, not just "scrolling backward"** — `_last_actual_scroll_offset.is_some() && (scrolling_backward || already_partially_revealed)`. A naive port that drops the "already partially revealed" disjunct (`_effective_scroll_offset < max_extent`) will fail to continue an in-progress reveal once the user resumes forward scrolling mid-reveal.
4. **`allow_floating_expansion`'s two sources of truth** — `constraints.user_scroll_direction == Forward` **or** `_last_started_scroll_direction == Some(Forward)` (`:661-664`) — the second disjunct exists specifically for pointer/wheel scrolling, which (per the oracle's own comment, `:524-526`) "does not have the same concept of a hold-and-release scroll movement." Dropping it silently breaks floating-reveal-on-reverse for trackpad/wheel input specifically — an easy gap to miss since drag-scroll testing alone won't catch it. (FLUI's `update_scroll_start_direction` is exposed but never internally called — §3 — so this field's value is entirely test-harness-driven for now; document that explicitly rather than silently defaulting it.)
5. **`max_scroll_obstruction_extent` must be reported even though it looks unused locally** — it isn't consumed anywhere inside this render object; it's consumed by the *next* sliver via `SliverConstraints.overlap` (confirmed wired in FLUI's `RenderViewport`, §1). Omitting it (since nothing in the header's own tests would fail without it) silently breaks the sliver-stack interaction — the exact `overlapsContent`-interaction risk the task flagged in point 5. This needs a `viewport_multi`-style two-sliver harness test to catch (§6), not a single-sliver test.
6. **The edge-triggered stretch callback** (`:255-260`) — `stretch_offset >= trigger && _last_stretch_offset <= trigger`. A level-triggered naive port (fire whenever `stretch_offset >= trigger`) fires every frame while overscrolled past the threshold instead of once per crossing.
7. **`show_on_screen`'s coordinate space** — Pinned trims in the *sliver's* space; Floating trims in the *child's* space (oracle comment `:709-714` explains why: pinned headers can't sit above their normal position, floating ones can). Since `show_on_screen` is deferred entirely (no FLUI base method exists), this is currently moot — but flag it so whoever eventually adds `RenderObject::show_on_screen` to FLUI doesn't copy one variant's convention for both.

## 6. Test plan (harness rigor matching `RenderSliverGrid`/`RenderShrinkWrappingViewport`)

Pattern precedent: `crates/flui-objects/tests/render_object_harness.rs:3200-3277` (`harness_sliver_list_anchor_correction_forward_emits_backward_suppresses`) is the **exact template** for multi-scroll-offset sequencing — `RenderTester::mount(viewport_with_scroll(...))`, then `run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| { vp.offset_mut().set_pixels(X); vp.offset_mut().set_user_scroll_direction(Y); }); run.relayout();`, re-asserting `run.offset(...)`/`run.sliver_geometry(...)` after each pass. This gives a real, harness-native multi-frame scroll sequence with **no scheduler/vsync machinery needed** for the scroll-offset dimension (only the floating header's *snap animation*, if exercised, needs `Vsync::tick_all`-style driving, same mechanism `RenderAnimatedSize`'s tests already use).

Per-variant (each gets its own `harness_sliver_persistent_header_*` block):
- **Scrolling**: single-frame layout at several `scroll_offset` values (0, mid-shrink, at `min_extent`, past `min_extent`) asserting `child_extent`/`paint_extent`/`child_main_axis_position` against hand-computed oracle formulas; assert `has_visual_overflow` is always `true` (oracle's conservative constant).
- **Pinned**: assert `child_main_axis_position == 0.0` at every scroll offset (the pin); assert `max_scroll_obstruction_extent == min_extent`; a **two-sliver `viewport_multi` test** (pinned header + a following list) asserting the follower's `SliverConstraints.overlap` at the pinned header's committed `min_extent` once scrolled past shrink — this is the test that would fail if trap #5 is introduced.
- **Floating — re-reveal sequence** (the trickiest; mirror the anchor-correction test's multi-pass structure exactly):
  1. Scroll forward past `max_extent` (header fully shrunk/hidden) → assert `effective_scroll_offset == scroll_offset`, `child_main_axis_position` fully negative/hidden.
  2. Scroll backward a small amount with `user_scroll_direction = Reverse` → assert `effective_scroll_offset` decreases by exactly `delta` (immediate reveal, not gated on reaching the header's own visible region) — this is the core "float back into view" behavior.
  3. Repeat with `user_scroll_direction = Idle` but `last_started_scroll_direction` pre-seeded to `Forward` (call `update_scroll_start_direction` directly, since no caller wires it yet — §3) → assert the second disjunct of `allow_floating_expansion` still permits the reveal (trap #4's regression test).
  4. Continue scrolling forward again mid-reveal → assert `effective_scroll_offset` clamps back to `max_extent` per `:666-669`, not overshooting.
  5. Snap animation (only if constructed with a controller): drive via `Vsync::tick_all` (the `RenderAnimatedSize` test mechanism) after `maybe_start_snap_animation(Reverse)`, asserting `effective_scroll_offset` interpolates from its pre-snap value to `0.0`/`max_extent` over the configured duration, and `markNeedsLayout`/`mark_needs_layout` fires each tick the value changes (mirroring `RenderAnimatedSize`'s controller-listener test, not `_animation`).
  6. `attach` on a node with a live controller: assert the value-listener subscribes (matches `RenderAnimatedSize`'s attach/detach harness precedent from ADR-0013 Slice B's own milestone test).
- **FloatingPinned**: same re-reveal sequence as Floating (shared state machine — one shared test helper parametrized over both types would catch a copy/paste divergence), plus its distinct `update_geometry` clamp: assert `paint_extent` never drops below `min(min_extent, remaining_paint_extent)` even at full shrink (the "always at least min_extent visible, pinned" contract) and `child_main_axis_position` is **always `0.0`** even mid-reveal (unlike plain Floating, which can be negative).
- **Stretch** (shared base behavior, all variants): overscroll (`constraints.overlap < 0.0` at `scroll_offset == 0.0`) with a `stretch_configuration` present → assert child receives `max_extent + stretch_offset` as its constraint max; assert `on_stretch_trigger` fires exactly once across a sequence that crosses the trigger threshold and stays above it for several frames (trap #6's regression test), using an `Arc<AtomicUsize>` counter closure (same idiom as `RenderAnimatedSize`'s `on_end` test).
- **Dry layout / intrinsics**: not applicable — the oracle's base class exposes no `computeDryLayout`/intrinsic overrides beyond the inherited `RenderSliverHelpers` defaults; confirm FLUI's `Single`-arity sliver default forwarding is sufficient (no new work).
- **Catalog guard**: add all four names to `RENDER_OBJECT_TYPES` (`crates/flui-objects/tests/render_object_harness.rs:123-...`) plus a `docs/research/widget-renderobject-map.md` closure note mirroring the `RenderAnimatedSize`/`RenderCustomPaint` entries' format (§ "Remaining to build" table row `:53` gets removed/closed).

## 7. Deferred, documented (not silently dropped)

- Widget-layer `SliverPersistentHeader`/delegate/`SliverAppBar` — separate future pass; recommend modeling on `PinnedHeaderSliver`/`SliverFloatingHeader`'s simpler ordinary-child shape rather than the original delegate+rebuild-in-layout design (§2).
- `update_scroll_start_direction`/`maybe_start_snap_animation`/`maybe_stop_snap_animation` — exposed on the render object (harness-testable directly), wiring a real caller is `Scrollable`/`SliverAppBar`-layer work (§3).
- `show_on_screen` overrides — no base `RenderObject::show_on_screen` exists in FLUI at all yet; both overrides are inert until that infrastructure exists elsewhere in the catalog.
- `OverScrollHeaderStretchConfiguration.on_stretch_trigger`'s Dart `AsyncCallback` semantics — modeled as a synchronous `Arc<dyn Fn() + Send + Sync>` fire-and-forget (matching Dart's own fire-and-forget call site, which never awaits the returned `Future`, and consistent with FLUI's "no async in layout hot paths" contract).

### Critical Files for Implementation
- `crates/flui-objects/src/sliver/sliver_persistent_header.rs` (new — `PersistentHeaderCore`, `RenderSliverScrollingPersistentHeader`, `RenderSliverPinnedPersistentHeader`, `RenderSliverFloatingHeaderBase<M>`/`RenderSliverFloatingPersistentHeader`/`RenderSliverFloatingPinnedPersistentHeader`)
- `crates/flui-objects/src/sliver/sliver_to_box_adapter.rs` and `sliver_fill_remaining.rs` (existing — direct structural precedent for Single-arity Sliver-wrapping-Box-child, `SliverPhysicalParentData` usage, and the `child_paint_offset` reuse trick)
- `crates/flui-rendering/src/constraints/sliver_layout.rs` (existing `child_paint_offset` helper — reused via the `layout_offset := child_position + scroll_offset` substitution, §1)
- `crates/flui-rendering/src/traits/render_sliver.rs` (existing `attach`/`detach` defaults, `:463-478,610-616` — already-shipped ADR-0013 mechanism the Floating variant subscribes through)
- `crates/flui-objects/src/proxy/clip.rs` (existing — the `RenderClip<S: ClipGeometry>` generic-collapse precedent this design's `RenderSliverFloatingHeaderBase<M: FloatingHeaderMode>` follows)
- `crates/flui-objects/tests/render_object_harness.rs` (catalog registration + `harness_sliver_persistent_header_*` tests; multi-pass scroll-sequencing pattern at `:3200-3277` is the template)
