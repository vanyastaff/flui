//! How long a slot's frame ran on the GPU (ADR-0388).
//!
//! A frame is bracketed by two empty compute passes that write a timestamp
//! at their start: the first before the frame's clear, the second after its
//! last pass. The pair is resolved and copied into a readback buffer the
//! encoder maps once it is submitted, and a later frame of the slot reads the
//! ticks back, converts them with the queue's timestamp period and publishes
//! the [`GpuFrameTime`] of the frame's epoch. Nothing waits for the GPU: a
//! frame that finds every entry of the ring still in flight goes untimed.
//!
//! Only `Features::TIMESTAMP_QUERY` is needed, not
//! `TIMESTAMP_QUERY_INSIDE_ENCODERS`, since the timestamps are pass
//! timestamps. On a tile-based GPU (Apple, Mali) the passes between the two
//! timestamps are not guaranteed to run between them in order, so the time
//! is approximate there.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use flui_foundation::FrameEpoch;

use crate::raster_executor::GpuFrameTime;

/// How many frames of one slot can be timed while the GPU has not answered
/// the earliest: past it a frame goes untimed.
pub(crate) const GPU_TIMER_RING: usize = 3;

/// The two timestamps of one frame, eight bytes each.
const TIMESTAMPS: u32 = 2;
const RESOLVED_BYTES: wgpu::BufferAddress =
    TIMESTAMPS as wgpu::BufferAddress * wgpu::QUERY_SIZE as wgpu::BufferAddress;

/// What the readback's map callback reported, written from whichever thread
/// polls the device (wgpu's callbacks are `Send`).
const MAP_PENDING: u8 = 0;
const MAP_DONE: u8 = 1;
const MAP_FAILED: u8 = 2;

/// Where one entry of the ring stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Use {
    Free,
    /// Encoded into the frame of `epoch`, not submitted yet. An entry still
    /// here when the slot's next frame begins belonged to a frame that never
    /// reached its submission: its encoder was dropped with the mapping.
    Encoded(FrameEpoch),
    /// Submitted with the frame of `epoch`: its readback maps once the GPU
    /// finished the frame.
    Submitted(FrameEpoch),
}

struct Entry {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    mapped: Arc<AtomicU8>,
    in_use: Use,
}

/// A timed frame's entry, from [`FrameGpuTimer::begin`] to
/// [`FrameGpuTimer::submitted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimedFrame(usize);

/// One slot's frame timer. Built for a device that has
/// `Features::TIMESTAMP_QUERY`, and dropped with the device or the slot.
pub(crate) struct FrameGpuTimer {
    entries: [Entry; GPU_TIMER_RING],
    /// Nanoseconds per timestamp tick (`Queue::get_timestamp_period`).
    period_ns: f64,
    /// Times read back and not taken yet, oldest first.
    ready: [Option<GpuFrameTime>; GPU_TIMER_RING],
}

impl FrameGpuTimer {
    /// A timer on `device`, or `None` when it cannot write timestamps.
    pub(crate) fn try_new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let entry = || Entry {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("FLUI Frame GPU Timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: TIMESTAMPS,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("FLUI Frame GPU Timestamps Resolve"),
                size: RESOLVED_BYTES,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("FLUI Frame GPU Timestamps Readback"),
                size: RESOLVED_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            mapped: Arc::new(AtomicU8::new(MAP_PENDING)),
            in_use: Use::Free,
        };
        Some(Self {
            entries: [entry(), entry(), entry()],
            period_ns: f64::from(queue.get_timestamp_period()),
            ready: [None; GPU_TIMER_RING],
        })
    }

    /// Start timing the frame of `epoch`, first in `encoder`; `None` while
    /// every entry is in flight.
    pub(crate) fn begin(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        epoch: FrameEpoch,
    ) -> Option<TimedFrame> {
        for entry in &mut self.entries {
            if matches!(entry.in_use, Use::Encoded(_)) {
                entry.in_use = Use::Free;
            }
        }
        let index = self
            .entries
            .iter()
            .position(|entry| entry.in_use == Use::Free)?;
        let entry = &mut self.entries[index];
        timestamp_pass(encoder, &entry.queries, 0);
        entry.in_use = Use::Encoded(epoch);
        Some(TimedFrame(index))
    }

    /// End the frame `timed` names, last in `encoder`: its second timestamp,
    /// the resolve, and the readback mapped once the encoder is submitted.
    pub(crate) fn end(&mut self, encoder: &mut wgpu::CommandEncoder, timed: TimedFrame) {
        let entry = &mut self.entries[timed.0];
        timestamp_pass(encoder, &entry.queries, 1);
        encoder.resolve_query_set(&entry.queries, 0..TIMESTAMPS, &entry.resolve, 0);
        encoder.copy_buffer_to_buffer(&entry.resolve, 0, &entry.readback, 0, RESOLVED_BYTES);
        entry.mapped.store(MAP_PENDING, Ordering::Release);
        let mapped = Arc::clone(&entry.mapped);
        encoder.map_buffer_on_submit(&entry.readback, wgpu::MapMode::Read, .., move |result| {
            let state = if result.is_ok() { MAP_DONE } else { MAP_FAILED };
            mapped.store(state, Ordering::Release);
        });
    }

    /// The encoder holding `timed` was submitted.
    pub(crate) fn submitted(&mut self, timed: TimedFrame) {
        let entry = &mut self.entries[timed.0];
        if let Use::Encoded(epoch) = entry.in_use {
            entry.in_use = Use::Submitted(epoch);
        }
    }

    /// Read back every submitted frame the GPU has finished, into the times
    /// [`Self::take`] hands out. A readback that failed to map (the device
    /// was lost) frees its entry with no time.
    pub(crate) fn collect(&mut self) {
        for index in 0..GPU_TIMER_RING {
            let entry = &mut self.entries[index];
            let Use::Submitted(epoch) = entry.in_use else {
                continue;
            };
            let time = match entry.mapped.load(Ordering::Acquire) {
                MAP_DONE => {
                    let ticks = entry
                        .readback
                        .slice(..)
                        .get_mapped_range()
                        .ok()
                        .and_then(|bytes| ticks_between(&bytes));
                    entry.readback.unmap();
                    ticks.and_then(|ticks| duration_of(ticks, self.period_ns))
                }
                MAP_FAILED => None,
                _ => continue,
            };
            entry.in_use = Use::Free;
            if let Some(gpu) = time {
                self.publish(GpuFrameTime::new(epoch, gpu));
            }
        }
    }

    /// The oldest time read back and not taken yet.
    pub(crate) fn take(&mut self) -> Option<GpuFrameTime> {
        let taken = self.ready[0].take();
        self.ready.rotate_left(1);
        taken
    }

    /// Queue `time` for [`Self::take`], oldest epoch first. A queue nobody
    /// took from drops its oldest time for the new one.
    fn publish(&mut self, time: GpuFrameTime) {
        if self.ready.iter().all(Option::is_some) {
            let _oldest = self.take();
        }
        if let Some(free) = self.ready.iter_mut().find(|ready| ready.is_none()) {
            *free = Some(time);
        }
        self.ready
            .sort_unstable_by_key(|ready| ready.map_or(u64::MAX, |time| time.epoch.get()));
    }
}

/// An empty compute pass that writes timestamp `index` of `queries` at its
/// start: a pass timestamp, which needs no `TIMESTAMP_QUERY_INSIDE_ENCODERS`.
fn timestamp_pass(encoder: &mut wgpu::CommandEncoder, queries: &wgpu::QuerySet, index: u32) {
    let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("FLUI Frame GPU Timestamp"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: queries,
            beginning_of_pass_write_index: Some(index),
            end_of_pass_write_index: None,
        }),
    });
}

/// The ticks from the first to the second of the two little-endian
/// timestamps in `resolved`; `None` when the second is earlier (a counter
/// that wrapped or was reset between the passes).
fn ticks_between(resolved: &[u8]) -> Option<u64> {
    let tick = |at: usize| -> Option<u64> {
        let word: [u8; 8] = resolved.get(at..at + 8)?.try_into().ok()?;
        Some(u64::from_le_bytes(word))
    };
    tick(8)?.checked_sub(tick(0)?)
}

/// `ticks` of `period_ns` nanoseconds each, or `None` when the product is not
/// a time (a period of zero or one that is not finite, a value past
/// `Duration::MAX`).
fn duration_of(ticks: u64, period_ns: f64) -> Option<std::time::Duration> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame's ticks are far below 2^53, where f64 is exact"
    )]
    let nanos = ticks as f64 * period_ns;
    if !(nanos.is_finite() && period_ns > 0.0) {
        return None;
    }
    std::time::Duration::try_from_secs_f64(nanos / 1e9).ok()
}

#[cfg(test)]
mod tests {
    use super::duration_of;

    #[test]
    fn ticks_convert_through_the_queues_period() {
        assert_eq!(
            duration_of(1_000, 1.0),
            Some(std::time::Duration::from_micros(1))
        );
        assert_eq!(
            duration_of(500, 40.0),
            Some(std::time::Duration::from_micros(20)),
            "a 40 ns tick, as some mobile GPUs report"
        );
        assert_eq!(duration_of(10, 0.0), None, "no period, no time");
        assert_eq!(duration_of(10, f64::NAN), None);
    }
}

#[cfg(test)]
mod frame_tests {
    use flui_foundation::geometry::Rect;
    use flui_layer::{DamageRegion, FullReason, Scene, SceneBuilder};
    use flui_painting::{Canvas, paint::Paint, styling::Color};

    use super::GPU_TIMER_RING;

    fn square() -> Scene {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            &Paint::fill(Color::rgb(255, 0, 0)),
        );
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        Scene::new(builder.build())
    }

    /// The window path's frame protocol times its frames: each frame is read
    /// back by a later one under its own epoch. A frame that fails or panics
    /// before its submission is not timed and gives its entry back, so the
    /// frames after it are timed again, however many fail first.
    ///
    /// Red-check: keep an entry `Encoded` at `begin` instead of freeing it
    /// and, after a ring's worth of failed frames, no frame is timed again.
    #[test]
    fn frames_that_never_submit_give_their_entries_back() {
        let Some(renderer) = crate::test_support::renderer_or_skip() else {
            return;
        };
        let mut capture = renderer.retained_capture((16, 16)).expect("capture");
        let scene = square();
        let mut times = Vec::new();
        let mut draw = |capture: &mut crate::headless::RetainedCapture| {
            while let Some(time) = capture.take_gpu_time() {
                times.push(time.epoch.get());
            }
            capture.apply_damage(DamageRegion::Full(FullReason::FirstFrame));
            let drawn = capture.render_scene(&scene);
            capture
                .read_rgba()
                .expect("the GPU finishes the frame before the next one");
            drawn
        };
        // Epochs 1 and 2 draw, the next RING + 1 fail before their
        // submission and one more panics there, then two draw.
        draw(&mut capture).expect("frame 1 draws");
        draw(&mut capture).expect("frame 2 draws");
        for _ in 0..=GPU_TIMER_RING {
            capture.fail_next_frame_after_record();
            assert!(draw(&mut capture).is_err(), "the injected failure fails");
        }
        capture.panic_after_next_record();
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the test holds the raster frame's own panic"
        )]
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw(&mut capture)));
        assert!(unwound.is_err(), "the injected panic unwinds the frame");
        draw(&mut capture).expect("the next frame draws");
        draw(&mut capture).expect("and the one after");
        while let Some(time) = capture.take_gpu_time() {
            times.push(time.epoch.get());
        }
        let failed = u64::try_from(GPU_TIMER_RING).expect("a small ring") + 2;
        if capture.times_frames() {
            assert_eq!(
                times,
                [1, 2, 3 + failed],
                "the drawn frames are timed, the failed and the unwound are not, and the last \
                 is read back by none yet"
            );
        } else {
            assert!(
                times.is_empty(),
                "a device without timestamps times nothing"
            );
        }
    }
}
