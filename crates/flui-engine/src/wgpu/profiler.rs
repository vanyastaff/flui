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
//! owned as an `Option<GpuFrameProfiler>` by [`crate::wgpu::Renderer`]. When the
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
pub struct GpuFrameProfiler {
    inner: wgpu_profiler::GpuProfiler,
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
    pub fn new(device: &wgpu::Device) -> Result<Self, wgpu_profiler::CreationError> {
        let settings = wgpu_profiler::GpuProfilerSettings {
            enable_timer_queries: true,
            enable_debug_groups: true,
            max_num_pending_frames: Self::PENDING_FRAME_BUFFER_DEPTH,
        };
        Ok(Self {
            inner: wgpu_profiler::GpuProfiler::new(device, settings)?,
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
    pub fn scope<'a>(
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
    pub fn resolve_queries(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.inner.resolve_queries(encoder);
    }

    /// Signal the end of a GPU frame.
    ///
    /// Call after all submits for the current frame. Errors (unclosed/unresolved
    /// queries) are logged via `tracing` rather than propagated — a profiling
    /// error must never abort a frame.
    pub fn end_frame(&mut self) {
        if let Err(err) = self.inner.end_frame() {
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
    pub fn process_finished_frame(&mut self, timestamp_period: f32) -> Option<&GpuFrameProfile> {
        if let Some(raw_results) = self.inner.process_finished_frame(timestamp_period) {
            let mut passes = Vec::with_capacity(raw_results.len());
            flatten_timer_results(&raw_results, 0, &mut passes);
            self.latest_profile = Some(GpuFrameProfile { passes });
        }
        self.latest_profile.as_ref()
    }

    /// The latest completed frame profile, or `None` if no frame has resolved.
    #[must_use]
    pub fn latest_completed_frame(&self) -> Option<&GpuFrameProfile> {
        self.latest_profile.as_ref()
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
pub struct ScopeGuard<'a> {
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
    pub fn recorder(&mut self) -> &mut wgpu::CommandEncoder {
        self.inner.recorder
    }
}

// ScopeGuard::drop closes the wgpu_profiler::Scope automatically (its Drop
// impl calls GpuProfiler::end_query). Nothing additional required here.

// ---------------------------------------------------------------------------
// Tests — feature-independent (no GPU, no wgpu-profiler required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{GpuFrameProfile, PassTiming};
    use flui_foundation::Diagnosticable;

    /// Builds a synthetic `GpuFrameProfile` from hand-crafted `PassTiming` data
    /// and asserts that `debug_fill_properties` emits the expected property names
    /// and formatted values. This test requires no GPU and no `wgpu-profiler` —
    /// it proves only the flui-owned mapping logic, per Fix #8 (anti-fake-pass).
    #[test]
    fn diagnosticable_maps_pass_timings_to_properties() {
        let profile = GpuFrameProfile {
            passes: vec![
                PassTiming {
                    label: "Clear Pass".into(),
                    duration_ms: 0.123,
                    depth: 0,
                },
                PassTiming {
                    label: "Nested Scope".into(),
                    duration_ms: 0.456,
                    depth: 1,
                },
                PassTiming {
                    label: "Final Render".into(),
                    duration_ms: 2.789,
                    depth: 0,
                },
            ],
        };

        let node = profile.to_diagnostics_node();
        let props = node.properties();

        // Must have one entry per pass + the "total" summary.
        assert_eq!(
            props.len(),
            4,
            "expected 3 pass properties + 1 total, got {}: {props:?}",
            props.len()
        );

        // Top-level passes have no indent prefix.
        assert_eq!(props[0].name(), "Clear Pass");
        assert_eq!(props[0].value(), "0.123ms");

        // Nested passes carry two-space indent per depth level.
        assert_eq!(props[1].name(), "  Nested Scope");
        assert_eq!(props[1].value(), "0.456ms");

        assert_eq!(props[2].name(), "Final Render");
        assert_eq!(props[2].value(), "2.789ms");

        // Total sums only depth-0 passes: 0.123 + 2.789 = 2.912.
        assert_eq!(props[3].name(), "total");
        assert_eq!(props[3].value(), "2.912ms");
    }

    #[test]
    fn total_ms_sums_only_top_level_passes() {
        let profile = GpuFrameProfile {
            passes: vec![
                PassTiming {
                    label: "A".into(),
                    duration_ms: 1.0,
                    depth: 0,
                },
                PassTiming {
                    label: "A/nested".into(),
                    duration_ms: 0.5,
                    depth: 1,
                },
                PassTiming {
                    label: "B".into(),
                    duration_ms: 2.0,
                    depth: 0,
                },
            ],
        };
        // Only depth-0 contribute: 1.0 + 2.0 = 3.0.
        assert!(
            (profile.total_ms() - 3.0).abs() < 1e-9,
            "total_ms() = {}, expected 3.0",
            profile.total_ms()
        );
    }

    #[test]
    fn empty_profile_has_zero_total() {
        let profile = GpuFrameProfile::default();
        assert!(
            profile.total_ms().abs() < f64::EPSILON,
            "total_ms() on empty profile = {}, expected 0.0",
            profile.total_ms()
        );
    }
}

// ---------------------------------------------------------------------------
// GPU-live test — exercises the real wgpu-profiler wiring (not synthetic data)
// ---------------------------------------------------------------------------
//
// Gated on `enable-wgpu-tests` (real GPU required) AND `gpu-profiler` (the
// feature under test). This is the anti-MVP / anti-fake-pass test: it FAILS
// under the BLOCKER (encoder-level scopes on an adapter without INSIDE_ENCODERS
// produce an empty profile) and PASSES after the fix on a capable adapter.
//
// Run with:
//   cargo test -p flui-engine \
//     --features enable-wgpu-tests,gpu-profiler -- --test-threads 1

#[cfg(all(test, feature = "enable-wgpu-tests", feature = "gpu-profiler"))]
mod gpu_live_tests {
    use super::{GpuFrameProfile, GpuFrameProfiler};

    /// Acquire a real wgpu adapter + device that requests TIMESTAMP_QUERY and
    /// TIMESTAMP_QUERY_INSIDE_ENCODERS, intersected with what the adapter actually
    /// supports. Returns `None` when no adapter is available (headless CI), and
    /// returns `(adapter, device, queue, has_inside_encoders)` otherwise.
    fn acquire_profiler_test_device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue, bool)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;

        let adapter_features = adapter.features();
        let has_inside_encoders =
            adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

        // Request both features intersected with what the adapter supports —
        // mirrors the production path in `GpuCapabilities::detect` /
        // `required_features`.
        let requested = (wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
            & adapter_features;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("GpuProfiler measures-test device"),
            required_features: requested,
            ..Default::default()
        }))
        .ok()?;

        Some((adapter, device, queue, has_inside_encoders))
    }

    /// Anti-MVP test: verify that the real wgpu-profiler wiring captures
    /// non-empty, finite timing results when the adapter supports encoder-level
    /// timestamp queries.
    ///
    /// Failure modes this catches:
    /// - Empty profile: profiler is `None` or `process_finished_frame` returns
    ///   `None` — the wiring never harvests results.
    /// - 0.0 ms timings: encoder-level scopes used without INSIDE_ENCODERS,
    ///   which is the BLOCKER this test validates the fix for.
    ///
    /// The test is explicitly skipped (not fake-passed) when the adapter lacks
    /// `TIMESTAMP_QUERY_INSIDE_ENCODERS`.
    #[test]
    fn profiler_captures_real_non_empty_timings_on_capable_adapter() {
        let Some((adapter, device, queue, has_inside_encoders)) = acquire_profiler_test_device()
        else {
            eprintln!("SKIP: no GPU adapter available in this environment");
            return;
        };

        if !has_inside_encoders {
            eprintln!(
                "SKIP: adapter lacks TIMESTAMP_QUERY_INSIDE_ENCODERS — \
                 encoder-level profiler scopes require it. adapter features: {:?}",
                adapter.features()
            );
            return;
        }

        // Adapter is capable. Construct the profiler and run enough frames to
        // warm the pending-frame pipeline.
        let mut profiler = GpuFrameProfiler::new(&device)
            .expect("GpuFrameProfiler::new must succeed on a capable device");

        // PENDING_FRAME_BUFFER_DEPTH = 3; add 2 extra to ensure the oldest frame
        // has definitely resolved through the full pipeline before we assert.
        let warm_up_frames = 5_usize;

        // Acquire a small offscreen texture for the render passes to write into.
        // Without real GPU work inside the scope the driver may not generate a
        // meaningful timestamp delta; a real clear pass is the cheapest option.
        let target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("profiler-test-target"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut last_profile: Option<GpuFrameProfile> = None;

        for _frame in 0..warm_up_frames {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("profiler-test-encoder"),
            });

            // Open a named encoder-level scope — this is the exact path used in
            // production (renderer.rs `render_scene`). Without INSIDE_ENCODERS the
            // scope records 0.0 ms; with it, the timestamp pair flanks the work.
            let mut scope = profiler.scope("test_pass", &mut encoder);

            // Perform trivial real GPU work: clear the 4×4 texture. This ensures
            // the driver has something to timestamp.
            {
                let _pass = scope
                    .recorder()
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("profiler-test-clear"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &target_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                // _pass drops here, ending the render pass
            }
            // scope drops here → end_query fires (requires INSIDE_ENCODERS)
            drop(scope);

            profiler.resolve_queries(&mut encoder);
            queue.submit(std::iter::once(encoder.finish()));

            // Block until all submitted work (including the query resolve) has
            // completed before calling end_frame + process_finished_frame.
            // `wait_indefinitely()` blocks on the most-recent submission with
            // no timeout — correct for a synchronous test loop.
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("device poll failed");

            profiler.end_frame();

            let timestamp_period = queue.get_timestamp_period();
            if let Some(profile) = profiler.process_finished_frame(timestamp_period) {
                last_profile = Some(profile.clone());
            }
        }

        // After warm_up_frames frames at least one completed result must be
        // available. An empty profile here is the BLOCKER symptom.
        let profile = last_profile.expect(
            "profiler must have produced at least one completed frame profile \
             after PENDING_FRAME_BUFFER_DEPTH + 2 frames — empty profile indicates \
             the encoder-level scope is recording nothing (INSIDE_ENCODERS bug)",
        );

        eprintln!(
            "GpuProfiler captures-real-timings test: {} pass(es) recorded",
            profile.passes.len()
        );
        for pass in &profile.passes {
            eprintln!(
                "  depth={} label={:?} duration_ms={:.4}",
                pass.depth, pass.label, pass.duration_ms
            );
        }
        eprintln!("  total_ms = {:.4}", profile.total_ms());

        assert!(
            !profile.passes.is_empty(),
            "profile must contain at least one pass timing, got empty passes — \
             check that the wgpu-profiler scope wiring is correct"
        );

        let test_pass = profile
            .passes
            .iter()
            .find(|p| p.label == "test_pass")
            .expect(
                "profile must include a 'test_pass' entry — \
                 the scope label was not captured by wgpu-profiler",
            );

        assert!(
            test_pass.duration_ms.is_finite(),
            "test_pass duration must be finite, got {}",
            test_pass.duration_ms
        );

        // A finite, non-negative duration proves the wiring captured real results.
        // (0.0 is theoretically possible if the GPU completes in < 1 ns, but in
        // practice a render-pass clear on any real adapter takes measurable time.)
        assert!(
            test_pass.duration_ms >= 0.0,
            "test_pass duration must be non-negative, got {}",
            test_pass.duration_ms
        );
    }
}
