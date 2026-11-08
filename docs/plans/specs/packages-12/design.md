# Design: producer-owned frame telemetry

`flui_foundation::diagnostics` owns `FrameSpan`, a non-exhaustive typed enumeration with constant `target()`/`name()` mappings and string-based `of` classification. It covers frame, presentation, every captured pipeline and animation phase, layout-root diagnostics and the animation-controller identity. Shared runtime, presentation and period field constants live beside it. All scheduler, runtime, view, rendering and animation producers use this contract through their existing downward foundation dependency.

Devtools classifies tracing metadata through `FrameSpan`; its public `Phase` remains the profiler/display classification and delegates target/name lookup to the shared enum. `LayoutRoot` and newly added variants are not captured implicitly. Captured spans retain typed identity rather than independent string constants.

The empty `flui_sdk::hooks` module and `Timeline::record_frame_snapshots` are deleted. `flui-scheduler` remains an internal producer of snapshots for the runtime overlay, histograms and structured tracing event; only the SDK's direct dependency and dead public bridge disappear. The scheduler stays in package-parity's train patch inventory where transitively required.

Devtools subscriber examples use `tracing_subscriber::registry`, `FrameTimingLayer` and `tracing::subscriber::set_global_default`. Documentation and ADRs are rewritten to state this single tracing contract and the remaining live snapshot paths. A changelog records the added contract and removed dead API.
