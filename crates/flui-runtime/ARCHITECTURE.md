# flui-runtime Architecture

The frame runtime of [ADR-0083](../../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) §1: the
per-presentation frame machinery a UI realm drives, placed below the hosts
(`flui-app`'s runners, platform wiring and raster lane) and above the widget
spine. The realm core moves here in steps; the ADR's `## Migration` section
lists them and what each waits on.

## Invariants

- **No host, platform or GPU edge.** The crate's normal dependency closure
  names none of `flui-platform`, `winit`, `android-activity`, `ndk`,
  `windows`, `objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine` or
  `flui-app`: tier K's forbid set in the root
  `[workspace.metadata.flui.reach]`, checked by `cargo xtask reach` over
  every root build. A seam that needs a
  host type (the frame sink, the platform window) crosses as a trait this
  crate defines or one from `flui-platform-api`.
- **Internal.** Tier K, `tier-kind = "internal"`: nothing here is an embedder
  API (ADR-0027 §9). `flui-app` is the only normal dependent; the facade
  re-exports nothing from it.
- **Per presentation, never per process.** Every type here is owned by one
  presentation (`HeldPointerQueue`, `SemanticsHost`, the commit epoch); there
  is no static, thread-local or process-global state.
- **Test hooks stay behind `test-support`.** Items that exist for tests, or
  that have no production caller yet (`HeldPointerQueue::append`/`len`,
  `SemanticsHost::ensure_semantics`, `outstanding_handles` and
  `platform_semantics_enabled`, the announce/event delivery), compile
  only under `cfg(test)` or the `test-support` feature, which only dev edges
  enable. Wiring one into production removes its gate in the same change.

## Mapping decisions

### Flutter's binding mixins become a runtime crate below the hosts

Flutter composes its frame runtime from `WidgetsBinding`, `RendererBinding`,
`SemanticsBinding` and `SchedulerBinding` mixins on one process-wide
singleton that the embedder drives. FLUI's runtime is per realm and per
presentation (ADR-0027), and its host is not the only thing that drives
frames: a test driver pumps the same realm on a virtual clock. The runtime
therefore lives in its own crate that names no host type, and the hosts depend
on it. Semantics enablement follows: `SemanticsHost` is one per presentation
instead of `SemanticsBinding`'s single instance, so two windows never share an
enablement count or a platform callback. Pinned by
`app::presentation::tests::semantics_host_is_exclusive_to_this_presentation`
in `flui-app`.
