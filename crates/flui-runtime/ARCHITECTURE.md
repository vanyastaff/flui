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
- **Internal, and only the host depends on it.** Tier K,
  `tier-kind = "internal"`: nothing here is an embedder API (ADR-0027 §9)
  except the `execution` host-injection seam below.
  `allowed-dependents = ["flui-app"]` makes `flui-app` the only crate allowed a
  normal edge, checked by `cargo xtask workspace` (and pinned by its
  `the_runtime_admits_only_the_host_as_a_normal_dependent`). That rule is what
  keeps ADR-0047's "no library crate can reach the pools" true now that
  `ExecutionServices` is `pub`; ADR-0083 §4 adds `flui-testing` when the test
  driver runs the real frame. Dev edges are not restricted.
- **The execution host-injection seam carries the Stable promise.**
  `HostExecutors`, `HostComputePool`, `HostIoPool`, `ComputeJob`, `IoFuture`,
  `SpawnError` and `DeterministicExecutors` are defined in `execution` but
  re-exported as `flui_app::…` and, through the facade's
  `pub use flui_app as app`, as `flui::app::…`; `AppConfig::with_executors`
  takes `HostExecutors`. The promise follows the item, not its crate's
  `tier-kind` (ADR-0089 §1), so a change to any of these signatures is a
  breaking change of `flui` (`SpawnError` is `#[non_exhaustive]`, so a new
  variant is not).
  The rest of `execution` (`ExecutionServices`) is reached
  only by `flui-app` and carries no promise. `execution_public_paths` in
  `flui-app` pins the re-exported paths.
- **Per presentation or per host loop, never per process.** Every type here is
  owned by one presentation (`HeldPointerQueue`, `SemanticsHost`,
  `PerformanceStats`, the commit epoch) or, for `ExecutionServices`, by one
  host loop, constructed only by the host's composition root. There is no
  static, thread-local or process-global state.
- **The frame sink is the host's, the verdict is the realm's.** A host
  implements `sink::FrameSink`; the realm reads its `SubmitVerdict` and
  classifies retry, device loss and not-shown (ADR-0068). The trait stays
  object-safe, because the realm drives it as `&mut dyn FrameSink`
  (pinned by `sink::tests::a_host_sink_is_driven_through_dyn_frame_sink`).
  `SubmitVerdict` stays exhaustive, never `#[non_exhaustive]`: a new variant
  must make the compiler name the realm's match site in `flui-app`, and a
  wildcard arm there would swallow it (pinned by the enum's doctest, which
  matches every variant from outside the crate).
- **Test hooks stay behind `test-support`.** Items that exist for tests, or
  that have no production caller yet (`HeldPointerQueue::append`/`len`,
  `SemanticsHost::ensure_semantics`, `outstanding_handles`,
  `ExecutionServices::with_limits`/`owns_default_pools`/`default_pools_started` and
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

### A submit returns a verdict

Flutter's `FlutterView.render(scene)` returns nothing: whether the engine
presented the frame, dropped it for a lost surface or lost the device is the
engine's business, and the framework never retries. `FrameSink::submit`
returns a `SubmitVerdict` instead, and the realm classifies it: a stale surface
or a lost device arms a retry and keeps the frame's input epochs, a frame that
rendered but could not be shown is retained rather than counted as done, and a
frame with nothing to present falls back to no-present pacing (ADR-0068). The
divergence predates this crate; it is recorded here because the verdict is now
a crate contract. Pinned by `flui-app`'s raster-lane classification tests, for
example `app::raster_lane::tests::a_withheld_frame_is_not_collapsed_into_no_present`.

`execution` has no Flutter counterpart to map: runtime and scheduling
topology, including background execution, is outside Flutter's reference
(ADR-0027), and ADR-0047 records its design.
