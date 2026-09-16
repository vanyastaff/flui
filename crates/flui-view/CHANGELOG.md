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
  per-frame re-entry budget (`MAX_MID_DRAIN_ABSORBS = 16`, shared across
  every drain the frame runs, including the layout-builder fixpoint's own
  passes) that charges only a self-rescheduling element landing a second
  time, never an independent element's first notification; exceeding it
  defers the leftover id to the next frame and logs one `tracing::warn!` per
  streak. See `ARCHITECTURE.md`'s `## Mapping decisions` for the two
  documented Flutter divergences (no descendant-only debug assert; a
  self-rescheduler rebuilds once per re-entry rather than being silently
  dropped) and the stale-`ElementId` hazard this makes likelier to observe.
