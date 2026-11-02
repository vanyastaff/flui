# Shared benchmark grid

Status: owner authorized the first FLUI implementation, baseline and measurement validation on 2026-11-02. External adapters and native latency are separate work.

- BG-1: One non-virtualized public widget scene supports 100, 1,000, 10,000 and 53,000 logical items, separately as labels and functioning buttons. Fixed logical viewport 1600×1000, scale 1, header 48, sidebar 260, grid cells 14, grid gap 2, outer padding 8. Sidebar rows are 22 high with gap 4; each pane clips overflow independently.
- BG-2: Report synchronous widget-pipeline wall time, including the state mutation. Retained no-change, local header change and forced reassemble are distinct. Never call this CPU process time, native frame latency, FPS or GPU time. Record first construction/mount separately.
- BG-3: Reuse HeadlessBinding and FrameReport. No second clock/driver or new library API. Serialize and inspect outside the timed operation. Preserve every sample and counter; reject existing output directories and unsupported input.
- BG-4: Verify exact logical text population, local state propagation, stable body, fixed geometry, clipping and button hits. Capture actual GPU output outside timing. Demonstrate that correctness and timing checks reject deliberate faults.
- BG-5: Baseline release binaries with a pinned source revision, lockfile, compiler, host, feature/profile settings and exact commands. Five independent repeats, at least 120 warmups and 10,000 samples per primary cell. Retain slow runs. A/A variation and unavailable measurements remain explicit; no performance pass/fail budget inferred from this host.
- BG-6: Publish implementation, verification evidence, baseline summaries and retrievable raw samples in the existing draft PR. Keep the draft unmerged; this does not approve the wider architecture roadmap.
