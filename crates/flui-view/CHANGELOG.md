# Changelog

All notable changes to `flui-view` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **`BuildOwner::drain_build_scope` absorbs the external-schedule inbox at
  the top of every heap pop, not only once at the start of the drain (issue
  #1180).** A build that calls another element's `RebuildHandle::schedule`
  synchronously now rebuilds the notified element in the SAME `build_scope`
  call instead of waiting a whole extra frame — e.g. a `Duration::ZERO`
  implicit-animation retarget's dependent `AnimatedBuilder` now settles on
  the same pump as the retargeting build, not the next one. Bounded by a new
  per-frame re-entry budget (`MAX_MID_DRAIN_ABSORBS = 16`; `build_scope`
  resets it once per real frame and factors into a non-resetting
  `build_scope_impl` so every mid-frame re-entrant caller — the
  layout-builder fixpoint's own passes and the lazy-sliver service pass's
  own second `build_scope` — shares the SAME budget rather than resetting
  it) that charges only a re-entry (an id notified again after it already
  completed a build this `build_scope` call — the same element
  rescheduling itself, a child notifying its already-built parent, or an
  A↔B ping-pong all count), never an independent element's first
  notification; exceeding it defers the leftover id to the next frame and
  logs one `tracing::warn!` per streak. See `ARCHITECTURE.md`'s
  `## Mapping decisions` for the three documented Flutter divergences (no
  descendant-only debug assert; a re-entered element rebuilds once per
  re-entry rather than being silently dropped; no latch suppressing the
  mid-drain frame request).

### Removed

- **The `RenderObjectElement` child-mutation seam (issue #1203).** The
  `RenderObjectElement` trait (its five child-mutation methods
  `attach_render_object` / `detach_render_object` /
  `insert_render_object_child` / `move_render_object_child` /
  `remove_render_object_child`, the type-erased `render_object_any`
  accessors, and `find_`/`set_ancestor_render_object_element`), the
  `RenderSlot` enum, and the seam's state on `RenderBehavior` (`slot`,
  `ancestor_render_object_element`, and their accessors) are deleted as
  dead code — zero production callers across the workspace; every Flutter
  consumer family of the seam maps to a live FLUI equivalent running the
  opposite direction (the child adopts itself at mount). The mapping
  decision is recorded in this crate's `ARCHITECTURE.md`. The
  `RenderTreeRootElement` marker trait stays: the root bootstrap it
  documents is live.
