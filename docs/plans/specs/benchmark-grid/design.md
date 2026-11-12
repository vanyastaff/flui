# Shared benchmark grid design

The example owns the workload and measurement adapter. Integration tests include the same source modules; there is no alternate test-only scene. GridScene owns the header StateHandle and the last activated typed-by-position item index. The callback is synchronous and stores only the item index in an owner-thread Cell.

Labels and buttons have the same text and bounds. Buttons use RawButton with focus and gesture owners. Every logical item is instantiated, including clipped rows. SingleChildScrollView is used for non-virtualized clipping, at offset zero. No scroll throughput is claimed. Static body views are retained inside one RepaintBoundary; only the Header binds to mutable state. This is a declared application optimization: a local header update must reuse the body's paint, while retaining every item. Forced reassemble is a diagnostic positive control, not an imitation of another framework's ordinary update.

The measured closure starts before mutation and ends after HeadlessBinding::pump_frame_report returns. Instant is the real elapsed clock. The driver's fixed 16,666,667 ns advances animation state only. Samples distinguish driver invocations from frames_produced; idle invocations can produce zero frames. Mount includes view construction, owners, build, layout and initial paint; it does not separate these phases or include process startup/GPU submission.

The raw-owner driver is intentional: it exposes existing counters without adding framework APIs. It does not execute UiRuntime, the native event loop, renderer submission or presentation. A future end-to-end adapter must retain the scene and use the actual runtime. HeadlessRenderer is used only for offscreen pixel evidence and includes blocking readback, outside benchmark timings.

Raw samples are collected in preallocated storage and written after the loop. Summary uses nearest-rank percentiles; run distributions are kept separate from between-process A/A variation. Existing output directories are rejected so failures cannot replace evidence. A VALID summary is written only after before/after scene checks succeed. GPU capture is optional for repeated timing runs; the evidence manifest identifies the separately validated captures.

Alternatives: Criterion alone would hide per-invocation FrameReport correlation; a new xtask driver would duplicate existing test owners; native FPS would conflate present pacing with pipeline work. Keep this first adapter small and explicit, then integrate new measurement boundaries when those capabilities exist.
