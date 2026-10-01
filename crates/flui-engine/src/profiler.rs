//! GPU frame profiler — optional wrapper around `wgpu-profiler` 0.27.
//!
//! Enabled by the `gpu-profiler` Cargo feature (off by default). Requires BOTH
//! `TIMESTAMP_QUERY` AND `TIMESTAMP_QUERY_INSIDE_ENCODERS` wgpu adapter features
//! at runtime. When the adapter supports only the base `TIMESTAMP_QUERY` but not
//! `INSIDE_ENCODERS`, the profiler stays `None` (no-op) — it never records
//! silent 0.0 ms timings, which would be the result of using encoder-level scopes
//! on an adapter that lacks `INSIDE_ENCODERS`.
//!
//! # Integration
//!
//! `GpuFrameProfiler` (a feature-gated type) wraps the underlying `wgpu_profiler::GpuProfiler` and is
//! owned as an `Option<GpuFrameProfiler>` by [`crate::Renderer`]. When the
//! option is `None` — either because the feature flag is off or the adapter lacks
//! `TIMESTAMP_QUERY` — every call site compiles away with no runtime cost.
//!
//! # Wasm
//!
//! This module is compiled on all targets, but `GpuFrameProfiler` is only
//! constructible when `wgpu-profiler` is available (i.e. the `gpu-profiler`
//! feature is enabled). The feature must NOT be enabled for `wasm32` targets;
//! see `crates/flui-engine/Cargo.toml`.
//!
//! # Design note — flui-owned timing record
//!
//! Rather than exposing `wgpu_profiler::GpuTimerQueryResult` in the
//! `Diagnosticable` impl (which would leak an optional dep into the public
//! diagnostic surface), completed timer results are mapped to [`PassTiming`],
//! a plain flui-owned struct. This keeps the `Diagnosticable` impl fully
//! unit-testable without a live GPU or the `gpu-profiler` feature.

use std::fmt;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};

// ---------------------------------------------------------------------------
// Flui-owned timing record (feature-independent, always compiled)
// ---------------------------------------------------------------------------

/// The GPU time measured for a single profiler scope (render/clear/flush pass).
///
/// Mapped from `wgpu_profiler::GpuTimerQueryResult`; stored in [`GpuFrameProfile`]
/// after each completed frame. All times are in milliseconds.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct PassTiming {
    /// Human-readable label matching the scope label passed to the profiler.
    pub label: String,
    /// Measured GPU duration in milliseconds. `0.0` when the adapter did not
    /// return a timestamp (e.g. driver withheld results for an in-flight frame).
    pub duration_ms: f64,
    /// Nesting depth (0 = top-level scope, 1 = nested, …).
    pub depth: u32,
}

/// A snapshot of GPU pass timings for a completed frame.
///
/// Implements [`Diagnosticable`] so it can be printed via the standard
/// diagnostic tree. Each pass becomes one property: `"label" = "X.XXms"`.
///
/// Produced by `GpuFrameProfiler::latest_completed_frame` (available with the
/// `gpu-profiler` feature) when a frame's queries have resolved. `None` means
/// no frame has completed yet (the profiler needs `max_num_pending_frames`
/// frames to warm up).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct GpuFrameProfile {
    /// Per-pass timing records, in submission order.
    pub passes: Vec<PassTiming>,
}

impl GpuFrameProfile {
    /// Returns the total GPU frame time in milliseconds (sum of all top-level
    /// pass durations, depth == 0).
    #[must_use]
    pub fn total_ms(&self) -> f64 {
        self.passes
            .iter()
            .filter(|pass| pass.depth == 0)
            .map(|pass| pass.duration_ms)
            .sum()
    }
}

impl fmt::Display for GpuFrameProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GpuFrameProfile(total={:.3}ms)", self.total_ms())
    }
}

impl Diagnosticable for GpuFrameProfile {
    fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
        for pass in &self.passes {
            let indent = "  ".repeat(pass.depth as usize);
            builder.add(
                format!("{}{}", indent, pass.label),
                format!("{:.3}ms", pass.duration_ms),
            );
        }
        builder.add("total", format!("{:.3}ms", self.total_ms()));
    }
}

// ---------------------------------------------------------------------------
// Recursive flattening of wgpu_profiler results → PassTiming
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu-profiler")]
fn flatten_timer_results(
    results: &[wgpu_profiler::GpuTimerQueryResult],
    depth: u32,
    out: &mut Vec<PassTiming>,
) {
    for result in results {
        let duration_ms = result
            .time
            .as_ref()
            .map_or(0.0, |range| (range.end - range.start) * 1_000.0);
        out.push(PassTiming {
            label: result.label.clone(),
            duration_ms,
            depth,
        });
        flatten_timer_results(&result.nested_queries, depth + 1, out);
    }
}

// ---------------------------------------------------------------------------
// GpuFrameProfiler — the feature-gated profiler handle
// ---------------------------------------------------------------------------

/// GPU frame profiler wrapping `wgpu_profiler::GpuProfiler`.
///
/// Only constructible when the `gpu-profiler` feature is enabled AND the adapter
/// exposes BOTH `wgpu::Features::TIMESTAMP_QUERY` AND
/// `wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`. The encoder-level scopes
/// opened by [`GpuFrameProfiler::scope`] require `INSIDE_ENCODERS`; without it
/// wgpu-profiler records 0.0 ms for every scope — a silent silent mis-measurement.
///
/// `Renderer` holds this as `Option<GpuFrameProfiler>`. `None` means profiling is
/// disabled (absent feature flag or incapable adapter) — all call sites are no-ops.
///
/// # Frame protocol
///
/// For each rendered frame:
/// 1. Wrap encoders in [`GpuFrameProfiler::scope`] (returns a `ScopeGuard`).
/// 2. Drop all scope guards before calling [`GpuFrameProfiler::resolve_queries`].
/// 3. Call `resolve_queries` on the last encoder **before** `queue.submit`.
/// 4. Call [`GpuFrameProfiler::end_frame`] after all submits for the frame.
/// 5. Optionally call [`GpuFrameProfiler::process_finished_frame`] after
///    `SurfaceTexture::present` to harvest the oldest completed result.
#[cfg(feature = "gpu-profiler")]
pub(crate) struct GpuFrameProfiler {
    inner: wgpu_profiler::GpuProfiler,
    /// An aborted frame may contain queries in encoders that were never submitted.
    /// Discard that profiler before the next frame instead of mapping those queries.
    abandoned: bool,
    /// The most recently harvested completed-frame profile, if any.
    latest_profile: Option<GpuFrameProfile>,
}

#[cfg(feature = "gpu-profiler")]
impl fmt::Debug for GpuFrameProfiler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuFrameProfiler")
            .field("has_latest_profile", &self.latest_profile.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "gpu-profiler")]
impl GpuFrameProfiler {
    /// Number of in-flight frames the profiler buffers before returning results.
    ///
    /// Three matches a typical triple-buffer pipeline. Lowering it reduces
    /// latency between GPU execution and result availability at the cost of
    /// more frequent pipeline stalls; raising it smooths bursty query
    /// resolution at the cost of staler data.
    const PENDING_FRAME_BUFFER_DEPTH: usize = 3;

    /// Create a new profiler for the given device.
    ///
    /// The caller must have already verified that the adapter exposes both
    /// `TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_ENCODERS` (via
    /// `GpuCapabilities::supports_timestamp_queries`) and requested both features
    /// in the `DeviceDescriptor`. Constructing a profiler without `INSIDE_ENCODERS`
    /// would result in 0.0 ms timings for every encoder-level scope.
    ///
    /// # Errors
    ///
    /// Propagates `wgpu_profiler::CreationError` when the settings are invalid.
    /// In practice only `InvalidMaxNumPendingFrames` (value < 1) can fire, which
    /// cannot happen with the `PENDING_FRAME_BUFFER_DEPTH` constant above.
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, wgpu_profiler::CreationError> {
        let settings = wgpu_profiler::GpuProfilerSettings {
            enable_timer_queries: true,
            enable_debug_groups: true,
            max_num_pending_frames: Self::PENDING_FRAME_BUFFER_DEPTH,
        };
        Ok(Self {
            inner: wgpu_profiler::GpuProfiler::new(device, settings)?,
            abandoned: false,
            latest_profile: None,
        })
    }

    /// Open a named profiler scope on the given encoder.
    ///
    /// The returned [`ScopeGuard`] wraps `wgpu_profiler::Scope` and closes the
    /// query on drop. Drop the guard **before** calling [`Self::resolve_queries`].
    ///
    /// # Lifetime
    ///
    /// The guard borrows `self` and the encoder for its lifetime, preventing any
    /// other mutable use of the encoder while the scope is open — matching the
    /// wgpu-profiler contract.
    pub(crate) fn scope<'a>(
        &'a self,
        label: impl Into<String>,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> ScopeGuard<'a> {
        ScopeGuard {
            inner: self.inner.scope(label, encoder),
        }
    }

    /// Copy query results into a resolve buffer on the given encoder.
    ///
    /// Must be called **after** all scope guards for this encoder have been
    /// dropped, and **before** the encoder is submitted via `queue.submit`.
    pub(crate) fn resolve_queries(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.inner.resolve_queries(encoder);
    }

    /// Signal the end of a GPU frame.
    ///
    /// Call after all submits for the current frame. Errors (unclosed/unresolved
    /// queries) are logged via `tracing` rather than propagated — a profiling
    /// error must never abort a frame.
    pub(crate) fn end_frame(&mut self) {
        if let Err(err) = self.inner.end_frame() {
            self.abandoned = true;
            tracing::warn!(
                error = ?err,
                "GpuFrameProfiler::end_frame reported an error; \
                 profiling data for this frame may be incomplete"
            );
        }
    }

    /// Harvest the oldest completed frame's results, if available.
    ///
    /// `timestamp_period` is `wgpu::Queue::get_timestamp_period()` — the
    /// conversion factor from raw GPU ticks to nanoseconds.
    ///
    /// Returns the completed profile and stores it in [`Self::latest_completed_frame`].
    /// Returns `None` when the GPU pipeline hasn't yet completed enough frames
    /// to return results (normally requires `PENDING_FRAME_BUFFER_DEPTH` frames).
    pub(crate) fn process_finished_frame(
        &mut self,
        timestamp_period: f32,
    ) -> Option<&GpuFrameProfile> {
        if self.abandoned {
            return self.latest_profile.as_ref();
        }
        if let Some(raw_results) = self.inner.process_finished_frame(timestamp_period) {
            let mut passes = Vec::with_capacity(raw_results.len());
            flatten_timer_results(&raw_results, 0, &mut passes);
            self.latest_profile = Some(GpuFrameProfile { passes });
        }
        self.latest_profile.as_ref()
    }

    /// The latest completed frame profile, or `None` if no frame has resolved.
    #[must_use]
    pub(crate) fn latest_completed_frame(&self) -> Option<&GpuFrameProfile> {
        self.latest_profile.as_ref()
    }
}

/// Owns a rendered frame's profiling obligation, including error and unwind exits.
/// Dropping an incomplete frame only marks it abandoned: no GPU calls or allocation
/// can replace the original failure during unwinding. The next frame replaces the
/// abandoned query state before opening scopes; earlier valid diagnostics survive.
#[cfg(feature = "gpu-profiler")]
pub(crate) struct ProfileFrame<'a> {
    profiler: &'a mut Option<GpuFrameProfiler>,
    complete: bool,
}

#[cfg(feature = "gpu-profiler")]
impl<'a> ProfileFrame<'a> {
    pub(crate) fn begin(profiler: &'a mut Option<GpuFrameProfiler>, device: &wgpu::Device) -> Self {
        if let Some(previous) = profiler.as_mut().filter(|value| value.abandoned) {
            let latest = previous.latest_profile.take();
            *previous =
                GpuFrameProfiler::new(device).expect("BUG: fixed profiler settings remain valid");
            previous.latest_profile = latest;
        }
        Self {
            profiler,
            complete: false,
        }
    }

    pub(crate) fn profiler(&mut self) -> &mut Option<GpuFrameProfiler> {
        self.profiler
    }

    pub(crate) fn complete(mut self) {
        if let Some(profiler) = self.profiler.as_mut() {
            profiler.end_frame();
        }
        self.complete = true;
    }
}

#[cfg(feature = "gpu-profiler")]
impl Drop for ProfileFrame<'_> {
    fn drop(&mut self) {
        if !self.complete
            && let Some(profiler) = self.profiler.as_mut()
        {
            profiler.abandoned = true;
        }
    }
}

// ---------------------------------------------------------------------------
// ScopeGuard — RAII wrapper for wgpu_profiler::Scope<CommandEncoder>
// ---------------------------------------------------------------------------

/// RAII scope guard produced by [`GpuFrameProfiler::scope`].
///
/// Calls `end_query` on drop, closing the GPU timestamp pair. Must be dropped
/// before [`GpuFrameProfiler::resolve_queries`] is called on the same encoder.
#[cfg(feature = "gpu-profiler")]
pub(crate) struct ScopeGuard<'a> {
    inner: wgpu_profiler::Scope<'a, wgpu::CommandEncoder>,
}

#[cfg(feature = "gpu-profiler")]
impl fmt::Debug for ScopeGuard<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopeGuard").finish_non_exhaustive()
    }
}

#[cfg(feature = "gpu-profiler")]
impl ScopeGuard<'_> {
    /// Returns a mutable reference to the underlying `CommandEncoder`.
    ///
    /// Use this to drive render passes, painters, or any other operation that
    /// needs `&mut CommandEncoder` while the profiler scope is active. The
    /// returned reference is a reborrow of the scope's internally-held encoder
    /// reference, so the borrow checker correctly prevents concurrent mutable
    /// access to the encoder outside this scope.
    pub(crate) fn recorder(&mut self) -> &mut wgpu::CommandEncoder {
        self.inner.recorder
    }
}

#[cfg(all(test, feature = "testing", feature = "gpu-profiler"))]
pub(crate) mod tests {
    use super::{GpuFrameProfiler, ProfileFrame};

    /// Private failure seam: abandoned encoders and unwind exits cannot be
    /// manufactured through the public diagnostic snapshot API.
    pub(crate) fn failed_frames_do_not_pollute_the_next_profile() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            apply_limit_buckets: false,
            ..Default::default()
        }))
        .expect("profiler recovery requires a GPU adapter");
        let timestamps =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let required_features = if adapter.features().contains(timestamps) {
            timestamps
        } else {
            // Labels still expose frame contamination on timestamp-less adapters.
            wgpu::Features::empty()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features,
            ..Default::default()
        }))
        .expect("profiler recovery device");
        let mut profiler = Some(GpuFrameProfiler::new(&device).expect("valid profiler settings"));
        let mut failed = Vec::new();
        for (name, resolve, submit, unwind) in [
            ("unresolved content", false, false, false),
            ("resolved encoder refused", true, false, false),
            ("submitted work then blit failure", true, true, false),
            ("unwind after query resolve", true, false, true),
        ] {
            let row = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut frame = ProfileFrame::begin(&mut profiler, &device);
                    let active = frame.profiler().as_mut().expect("profiler enabled");
                    let mut encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                    drop(active.scope("aborted", &mut encoder));
                    if resolve {
                        active.resolve_queries(&mut encoder);
                    }
                    if submit {
                        queue.submit([encoder.finish()]);
                    } else {
                        drop(encoder);
                    }
                    assert!(!unwind, "injected frame failure");
                    // Return without completing, just like a managed submission error.
                }));
                assert_eq!(failure.is_err(), unwind);
                // Repeated good frames also prove that pending query bookkeeping
                // does not keep a failed frame alive or eventually exhaust its ring.
                for _ in 0..8 {
                    let mut frame = ProfileFrame::begin(&mut profiler, &device);
                    let active = frame.profiler().as_mut().expect("profiler stays enabled");
                    let mut encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                    drop(active.scope("successful retry", &mut encoder));
                    active.resolve_queries(&mut encoder);
                    queue.submit([encoder.finish()]);
                    frame.complete();
                    device
                        .poll(wgpu::PollType::Wait {
                            submission_index: None,
                            timeout: Some(std::time::Duration::from_secs(10)),
                        })
                        .expect("profiler query completion");
                    let snapshot = profiler
                        .as_mut()
                        .expect("profiler remains available")
                        .process_finished_frame(queue.get_timestamp_period())
                        .expect("a submitted frame produces diagnostics");
                    assert_eq!(snapshot.passes.len(), 1, "failed frame must not add scopes");
                    assert_eq!(snapshot.passes[0].label, "successful retry");
                }
            }));
            if row.is_err() {
                failed.push(name);
            }
        }
        assert!(
            failed.is_empty(),
            "profiler recovery cases failed: {failed:?}"
        );
    }
}
