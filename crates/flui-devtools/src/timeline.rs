//! Timeline event tracking for FLUI applications
//!
//! Records and visualizes events over time for performance analysis and
//! debugging. Supports exporting to Chrome DevTools trace format for advanced
//! visualization.
//!
//! # Example
//!
//! ```rust
//! use flui_devtools::timeline::{EventCategory, Timeline};
//!
//! let mut timeline = Timeline::new();
//!
//! // Record events
//! {
//!     let _guard = timeline.record_event("Build Widget Tree", EventCategory::Build);
//!     // Your build code here
//! } // Event duration automatically recorded
//!
//! {
//!     let _guard = timeline.record_event("Layout", EventCategory::Layout);
//!     // Your layout code here
//! }
//!
//! // Get all events
//! let events = timeline.get_events();
//! for event in events {
//!     println!("{}: {:.2}ms", event.name, event.duration_ms());
//! }
//!
//! // Export to Chrome DevTools format
//! let json = timeline.export_chrome_trace();
//! std::fs::write("trace.json", json).unwrap();
//! // Load trace.json in chrome://tracing
//! ```

use std::sync::Arc;

use flui_scheduler::FrameSnapshot;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use web_time::{Duration, Instant};

/// Category for timeline events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventCategory {
    /// Frame event (entire frame)
    Frame,
    /// Build phase (widget tree construction)
    Build,
    /// Layout phase (size calculation)
    Layout,
    /// Paint phase (rendering)
    Paint,
    /// Custom user-defined event
    Custom,
}

impl EventCategory {
    /// Get the category name as a string
    pub fn name(&self) -> &str {
        match self {
            EventCategory::Frame => "Frame",
            EventCategory::Build => "Build",
            EventCategory::Layout => "Layout",
            EventCategory::Paint => "Paint",
            EventCategory::Custom => "Custom",
        }
    }

    /// Get the category color (for visualization)
    ///
    /// Returns a color in hex format suitable for Chrome DevTools.
    pub fn color(&self) -> &str {
        match self {
            EventCategory::Frame => "#FF6B6B",  // Red
            EventCategory::Build => "#4ECDC4",  // Teal
            EventCategory::Layout => "#FFE66D", // Yellow
            EventCategory::Paint => "#95E1D3",  // Mint
            EventCategory::Custom => "#A8E6CF", // Light green
        }
    }
}

/// Returns the current thread's [`ThreadId`].
///
/// Used as a serde `default` function for `TimelineEvent::thread_id`, which is
/// skipped during serialization and reconstructed on deserialization.
fn current_thread_id() -> std::thread::ThreadId {
    std::thread::current().id()
}

/// The Chrome-trace "args" object for `event`'s Begin event:
/// `{"category": ...}` plus every key of `event.args`, IF it is itself a
/// JSON object (a frame-telemetry event's shape — see
/// [`Timeline::record_frame_snapshots`]). An event recorded through
/// [`Timeline::record_event`]/[`Timeline::record_instant`] carries
/// `args: Value::Null`, which contributes nothing beyond `category` — the
/// pre-existing behavior every caller of `export_chrome_trace` before this
/// function existed already relied on.
fn begin_event_args(event: &TimelineEvent) -> serde_json::Value {
    let mut args = serde_json::Map::new();
    args.insert("category".to_string(), json!(event.category.name()));
    if let serde_json::Value::Object(extra) = &event.args {
        for (key, value) in extra {
            args.insert(key.clone(), value.clone());
        }
    }
    serde_json::Value::Object(args)
}

/// A single timeline event
///
/// `#[non_exhaustive]`: a future field is additive — this slice already
/// added one (`args`), a semver break for any external constructor; marking
/// it now stops the next addition from repeating that break.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TimelineEvent {
    /// Event name/description
    pub name: String,
    /// Start time (microseconds since timeline creation)
    pub start_micros: u128,
    /// Duration (microseconds)
    pub duration_micros: u128,
    /// Event category
    pub category: EventCategory,
    /// Extra structured data carried alongside this event — e.g. a
    /// frame-telemetry event's coalesced input ids and latencies (see
    /// [`Timeline::record_frame_snapshots`]). `Value::Null` for an event
    /// recorded through [`Timeline::record_event`]/[`Timeline::record_instant`],
    /// which carry no extra args of their own.
    #[serde(default)]
    pub args: serde_json::Value,
    /// Thread ID (for multi-threaded applications)
    ///
    /// Not serialized; restored to the deserializing thread's ID on load.
    #[serde(skip, default = "current_thread_id")]
    pub thread_id: std::thread::ThreadId,
}

impl TimelineEvent {
    /// Get duration in milliseconds
    pub fn duration_ms(&self) -> f64 {
        self.duration_micros as f64 / 1000.0
    }

    /// Get start time in milliseconds
    pub fn start_ms(&self) -> f64 {
        self.start_micros as f64 / 1000.0
    }

    /// Get duration as Duration
    pub fn duration(&self) -> Duration {
        Duration::from_micros(self.duration_micros as u64)
    }
}

/// RAII guard for recording an event
///
/// Automatically records the event duration when dropped.
#[must_use = "EventGuard does nothing if not held"]
#[derive(Debug)]
pub struct EventGuard {
    timeline: Arc<Mutex<TimelineInner>>,
    event_index: usize,
    start: Instant,
}

impl Drop for EventGuard {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let mut inner = self.timeline.lock();
        inner.end_event(self.event_index, duration);
    }
}

/// Internal timeline state
#[derive(Debug)]
struct TimelineInner {
    /// Timeline start time (for relative timestamps)
    start_time: Instant,
    /// All recorded events
    events: Vec<TimelineEvent>,
    /// Maximum number of events to keep
    max_events: usize,
}

impl TimelineInner {
    fn new(max_events: usize) -> Self {
        Self {
            start_time: Instant::now(),
            events: Vec::new(),
            max_events,
        }
    }

    fn start_event(&mut self, name: String, category: EventCategory) -> usize {
        let now = Instant::now();
        let start_micros = (now - self.start_time).as_micros();

        let event = TimelineEvent {
            name,
            start_micros,
            duration_micros: 0, // Will be filled in when event ends
            category,
            args: serde_json::Value::Null,
            thread_id: std::thread::current().id(),
        };

        self.push_event(event)
    }

    /// Push an already-fully-formed event (known start/duration/args up
    /// front, unlike [`Self::start_event`]'s RAII start-then-later-end
    /// shape) and apply the same capacity trim. Returns the pushed event's
    /// index, for symmetry with `start_event` — no caller currently needs
    /// it, since a fully-formed event is never later mutated via
    /// [`Self::end_event`].
    fn push_event(&mut self, event: TimelineEvent) -> usize {
        let index = self.events.len();
        self.events.push(event);

        // Trim old events if we exceed max
        if self.events.len() > self.max_events {
            self.events.drain(0..self.events.len() - self.max_events);
            // Adjust index after draining
            self.events.len() - 1
        } else {
            index
        }
    }

    fn end_event(&mut self, index: usize, duration: Duration) {
        if let Some(event) = self.events.get_mut(index) {
            event.duration_micros = duration.as_micros();
        }
    }

    fn get_events(&self) -> Vec<TimelineEvent> {
        self.events.clone()
    }

    fn clear(&mut self) {
        self.events.clear();
        self.start_time = Instant::now();
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }
}

/// Timeline for recording and visualizing events
///
/// Thread-safe timeline that records events with precise timing.
/// Events can be exported to Chrome DevTools trace format for visualization.
#[derive(Clone)]
pub struct Timeline {
    inner: Arc<Mutex<TimelineInner>>,
}

impl Timeline {
    /// Create a new timeline
    ///
    /// Events will be kept in memory up to a default limit (10,000 events).
    pub fn new() -> Self {
        Self::with_capacity(10_000)
    }

    /// Create a new timeline with custom event capacity
    ///
    /// # Arguments
    ///
    /// - `max_events`: Maximum number of events to keep in memory
    pub fn with_capacity(max_events: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TimelineInner::new(max_events))),
        }
    }

    /// Record an event with RAII guard
    ///
    /// Returns a guard that automatically records the event duration when
    /// dropped.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use flui_devtools::timeline::{Timeline, EventCategory};
    /// # let timeline = Timeline::new();
    /// {
    ///     let _guard = timeline.record_event("My Operation", EventCategory::Custom);
    ///     // Your code here
    /// } // Event duration recorded here
    /// ```
    pub fn record_event(&self, name: impl Into<String>, category: EventCategory) -> EventGuard {
        let mut inner = self.inner.lock();
        let event_index = inner.start_event(name.into(), category);
        let start = Instant::now();

        EventGuard {
            timeline: self.inner.clone(),
            event_index,
            start,
        }
    }

    /// Record an instant event (duration = 0)
    ///
    /// Use this for events that happen at a point in time rather than over a
    /// duration.
    pub fn record_instant(&self, name: impl Into<String>, category: EventCategory) {
        let mut inner = self.inner.lock();
        let event_index = inner.start_event(name.into(), category);
        inner.end_event(event_index, Duration::ZERO);
    }

    /// Record an event whose timing is already fully known — unlike
    /// [`Self::record_event`]'s RAII start-then-measure-on-drop shape,
    /// `start`/`duration` are supplied up front (e.g. a frame's
    /// already-computed segment span) and this call never reads the wall
    /// clock itself. `start` is converted to this timeline's own
    /// relative-microseconds basis the same way `record_event` does.
    pub fn record_completed_event(
        &self,
        name: impl Into<String>,
        category: EventCategory,
        start: Instant,
        duration: Duration,
        args: serde_json::Value,
    ) {
        let mut inner = self.inner.lock();
        let start_micros = start
            .saturating_duration_since(inner.start_time)
            .as_micros();
        inner.push_event(TimelineEvent {
            name: name.into(),
            start_micros,
            duration_micros: duration.as_micros(),
            category,
            args,
            thread_id: std::thread::current().id(),
        });
    }

    /// Record every produced-frame [`FrameSnapshot`] in `snapshots` (pulled
    /// from a presentation's own `FrameClock::frames_since`) as one
    /// Chrome-trace-compatible timeline event per frame, each carrying its
    /// coalesced input ids and (present − arrival) latencies as trace args
    /// — issue #556's exportable, per-input-attributed frame telemetry.
    /// Reuses this module's own [`Self::export_chrome_trace`] serializer;
    /// no second trace format exists anywhere in this crate.
    ///
    /// The event name embeds [`FrameSnapshot::presentation`], not just
    /// `frame_id`: a `frame_id` is scoped to the presentation whose clock
    /// minted it (each starts counting from 1), so two presentations'
    /// snapshots recorded into the SAME `Timeline` — a realm hosting more
    /// than one presentation, e.g. via `open_secondary_window` — would
    /// otherwise both name themselves "Frame 1", "Frame 2", … and collide
    /// in one exported trace file. The `presentation` field is ALSO carried
    /// in `args` (not just the name) so a consumer can filter/group by it
    /// without parsing the display string back apart.
    pub fn record_frame_snapshots(&self, snapshots: &[FrameSnapshot]) {
        for snapshot in snapshots {
            let inputs: Vec<serde_json::Value> = snapshot
                .latencies()
                .map(|(id, latency)| {
                    json!({
                        "input_epoch_id": id.get(),
                        "latency_us": latency.as_micros() as u64,
                    })
                })
                .collect();
            let args = json!({
                "presentation": snapshot.presentation.to_string(),
                "frame_id": snapshot.frame_id.to_string(),
                "present_outcome": format!("{:?}", snapshot.present_outcome),
                "inputs": inputs,
            });
            self.record_completed_event(
                format!("Frame {} [{}]", snapshot.frame_id, snapshot.presentation),
                EventCategory::Frame,
                snapshot.segment_start,
                snapshot.segment_span(),
                args,
            );
        }
    }

    /// Get all recorded events
    pub fn get_events(&self) -> Vec<TimelineEvent> {
        self.inner.lock().get_events()
    }

    /// Get events filtered by category
    pub fn get_events_by_category(&self, category: EventCategory) -> Vec<TimelineEvent> {
        self.inner
            .lock()
            .get_events()
            .into_iter()
            .filter(|e| e.category == category)
            .collect()
    }

    /// Get events within a time range
    ///
    /// # Arguments
    ///
    /// - `start_ms`: Start time in milliseconds (relative to timeline start)
    /// - `end_ms`: End time in milliseconds (relative to timeline start)
    pub fn get_events_in_range(&self, start_ms: f64, end_ms: f64) -> Vec<TimelineEvent> {
        let start_micros = (start_ms * 1000.0) as u128;
        let end_micros = (end_ms * 1000.0) as u128;

        self.inner
            .lock()
            .get_events()
            .into_iter()
            .filter(|e| {
                let event_end = e.start_micros + e.duration_micros;
                e.start_micros >= start_micros && event_end <= end_micros
            })
            .collect()
    }

    /// Clear all events
    pub fn clear(&self) {
        self.inner.lock().clear();
    }

    /// Get the number of recorded events
    pub fn event_count(&self) -> usize {
        self.inner.lock().event_count()
    }

    /// Export events to Chrome DevTools trace format
    ///
    /// Returns a JSON string that can be loaded in chrome://tracing for
    /// visualization.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use flui_devtools::timeline::Timeline;
    /// # let timeline = Timeline::new();
    /// let json = timeline.export_chrome_trace();
    /// std::fs::write("trace.json", json).unwrap();
    /// // Then open chrome://tracing and load trace.json
    /// ```
    pub fn export_chrome_trace(&self) -> String {
        let events = self.get_events();

        let trace_events: Vec<_> = events
            .iter()
            .flat_map(|event| {
                // Chrome trace format uses "B" (begin) and "E" (end) events
                let thread_id = format!("{:?}", event.thread_id);

                vec![
                    // Begin event
                    json!({
                        "name": event.name,
                        "cat": event.category.name(),
                        "ph": "B", // Begin
                        "ts": event.start_micros,
                        "pid": 1,
                        "tid": thread_id,
                        "args": begin_event_args(event),
                    }),
                    // End event
                    json!({
                        "name": event.name,
                        "cat": event.category.name(),
                        "ph": "E", // End
                        "ts": event.start_micros + event.duration_micros,
                        "pid": 1,
                        "tid": thread_id,
                    }),
                ]
            })
            .collect();

        json!({
            "traceEvents": trace_events,
            "displayTimeUnit": "ms",
            "systemTraceEvents": "SystemTraceData",
            "otherData": {
                "version": "FLUI DevTools Timeline"
            }
        })
        .to_string()
    }

    /// Export events to a simple JSON format
    ///
    /// This is a simpler format than Chrome trace, useful for custom
    /// visualization.
    pub fn export_json(&self) -> String {
        let events = self.get_events();
        serde_json::to_string_pretty(&events).unwrap_or_default()
    }

    /// Print a summary of events
    pub fn print_summary(&self) {
        let events = self.get_events();

        println!("=== Timeline Summary ===");
        println!("Total events: {}", events.len());

        if events.is_empty() {
            return;
        }

        // Group by category
        let mut by_category: std::collections::HashMap<EventCategory, Vec<&TimelineEvent>> =
            std::collections::HashMap::new();

        for event in &events {
            by_category.entry(event.category).or_default().push(event);
        }

        for (category, category_events) in by_category {
            let total_ms: f64 = category_events.iter().map(|e| e.duration_ms()).sum();
            let avg_ms = total_ms / category_events.len() as f64;

            println!("\n{} ({} events):", category.name(), category_events.len());
            println!("  Total: {total_ms:.2}ms");
            println!("  Average: {avg_ms:.2}ms");

            // Show longest events
            let mut sorted = category_events.clone();
            sorted.sort_by_key(|e| std::cmp::Reverse(e.duration_micros));

            println!("  Longest events:");
            for event in sorted.iter().take(3) {
                println!("    {}: {:.2}ms", event.name, event.duration_ms());
            }
        }
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Timeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timeline")
            .field("event_count", &self.inner.lock().event_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn test_timeline_creation() {
        let timeline = Timeline::new();
        assert_eq!(timeline.event_count(), 0);
    }

    #[test]
    fn test_record_event() {
        let timeline = Timeline::new();

        {
            let _guard = timeline.record_event("Test Event", EventCategory::Custom);
            thread::sleep(Duration::from_millis(10));
        }

        let events = timeline.get_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "Test Event");
        assert!(events[0].duration_ms() >= 10.0);
    }

    #[test]
    fn test_record_instant() {
        let timeline = Timeline::new();

        timeline.record_instant("Instant Event", EventCategory::Custom);

        let events = timeline.get_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].duration_micros, 0);
    }

    #[test]
    fn test_multiple_events() {
        let timeline = Timeline::new();

        {
            let _guard = timeline.record_event("Event 1", EventCategory::Build);
            thread::sleep(Duration::from_millis(5));
        }

        {
            let _guard = timeline.record_event("Event 2", EventCategory::Layout);
            thread::sleep(Duration::from_millis(5));
        }

        {
            let _guard = timeline.record_event("Event 3", EventCategory::Paint);
            thread::sleep(Duration::from_millis(5));
        }

        let events = timeline.get_events();
        assert_eq!(events.len(), 3);

        // Verify order
        assert_eq!(events[0].name, "Event 1");
        assert_eq!(events[1].name, "Event 2");
        assert_eq!(events[2].name, "Event 3");
    }

    #[test]
    fn test_get_events_by_category() {
        let timeline = Timeline::new();

        timeline.record_instant("Build 1", EventCategory::Build);
        timeline.record_instant("Layout 1", EventCategory::Layout);
        timeline.record_instant("Build 2", EventCategory::Build);

        let build_events = timeline.get_events_by_category(EventCategory::Build);
        assert_eq!(build_events.len(), 2);
        assert_eq!(build_events[0].name, "Build 1");
        assert_eq!(build_events[1].name, "Build 2");
    }

    #[test]
    fn test_clear() {
        let timeline = Timeline::new();

        timeline.record_instant("Event 1", EventCategory::Custom);
        timeline.record_instant("Event 2", EventCategory::Custom);

        assert_eq!(timeline.event_count(), 2);

        timeline.clear();

        assert_eq!(timeline.event_count(), 0);
    }

    #[test]
    fn test_capacity_limit() {
        let timeline = Timeline::with_capacity(5);

        // Record more than capacity
        for i in 0..10 {
            timeline.record_instant(format!("Event {i}"), EventCategory::Custom);
        }

        // Should only keep last 5
        let events = timeline.get_events();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].name, "Event 5");
        assert_eq!(events[4].name, "Event 9");
    }

    #[test]
    fn test_export_json() {
        let timeline = Timeline::new();

        timeline.record_instant("Test Event", EventCategory::Build);

        let json = timeline.export_json();
        assert!(json.contains("Test Event"));
        assert!(json.contains("Build"));
    }

    #[test]
    fn test_export_chrome_trace() {
        let timeline = Timeline::new();

        timeline.record_instant("Test Event", EventCategory::Layout);

        let json = timeline.export_chrome_trace();
        assert!(json.contains("Test Event"));
        assert!(json.contains("\"ph\":\"B\"")); // Begin event
        assert!(json.contains("\"ph\":\"E\"")); // End event
        assert!(json.contains("traceEvents"));
    }

    #[test]
    fn test_nested_events() {
        let timeline = Timeline::new();

        {
            let _guard1 = timeline.record_event("Outer", EventCategory::Frame);
            thread::sleep(Duration::from_millis(5));

            {
                let _guard2 = timeline.record_event("Inner", EventCategory::Build);
                thread::sleep(Duration::from_millis(3));
            }

            thread::sleep(Duration::from_millis(2));
        }

        let events = timeline.get_events();
        assert_eq!(events.len(), 2);

        // Outer should be longer than inner
        let outer = events.iter().find(|e| e.name == "Outer").unwrap();
        let inner = events.iter().find(|e| e.name == "Inner").unwrap();

        assert!(outer.duration_ms() > inner.duration_ms());
    }

    #[test]
    fn test_thread_safety() {
        let timeline = Timeline::new();
        let timeline_clone = timeline.clone();

        let handle = thread::spawn(move || {
            timeline_clone.record_instant("Thread Event", EventCategory::Custom);
        });

        handle.join().unwrap();

        assert_eq!(timeline.event_count(), 1);
    }

    // ------------------------------------------------------------------
    // Frame telemetry export (issue #556): reuse, not a second format.
    // ------------------------------------------------------------------

    /// A real `FrameClock`-produced [`FrameSnapshot`] (driven through
    /// `flui_scheduler`'s own public API, not a hand-built stub — this
    /// crate has no way to construct one otherwise, since `InputEpochs`'
    /// fields are private) exports as valid Chrome trace JSON via THIS
    /// module's existing `export_chrome_trace`, with the coalesced input's
    /// id and latency actually present in the args — value, not merely a
    /// field that exists.
    #[test]
    fn frame_snapshot_export_carries_real_input_attribution_through_the_existing_serializer() {
        use flui_scheduler::{
            ClockSource, DemandKind, FrameClock, PollDecision, PresentOutcome, PresentationId,
        };

        let clock = FrameClock::with_source(ClockSource::Platform);
        let arrival = clock.now();
        let epoch_id = clock.stamp_input_epoch(arrival);

        clock.mark_demand(DemandKind::Dirty);
        let now = clock.now();
        assert_eq!(clock.poll(now), PollDecision::Produce);
        let submit_at = clock.now();
        let _snapshot = clock.record_frame(
            PresentationId::new(1),
            now,
            now,
            now,
            submit_at,
            PresentOutcome::Presented,
        );

        let snapshots = clock.frames_since(None);
        assert_eq!(snapshots.len(), 1);

        let timeline = Timeline::new();
        timeline.record_frame_snapshots(&snapshots);
        assert_eq!(
            timeline.event_count(),
            1,
            "one FrameSnapshot must become exactly one timeline event"
        );

        let json_text = timeline.export_chrome_trace();
        let parsed: serde_json::Value =
            serde_json::from_str(&json_text).expect("export_chrome_trace must produce valid JSON");
        let trace_events = parsed["traceEvents"]
            .as_array()
            .expect("traceEvents must be an array");
        assert_eq!(
            trace_events.len(),
            2,
            "one Begin + one End event per recorded frame, the same shape every other \
             Timeline event already uses"
        );

        let begin = &trace_events[0];
        assert_eq!(begin["ph"], "B");
        let inputs = begin["args"]["inputs"]
            .as_array()
            .expect("frame args must carry an inputs array");
        assert_eq!(
            inputs.len(),
            1,
            "the stamped epoch must survive into the export"
        );
        assert_eq!(
            inputs[0]["input_epoch_id"].as_u64(),
            Some(epoch_id.get()),
            "the exported record must name the SPECIFIC input id, not just report a count"
        );
        assert!(
            inputs[0]["latency_us"].as_u64().is_some(),
            "a latency value must be present, not merely a None/absent field"
        );
        assert_eq!(begin["args"]["frame_id"], snapshots[0].frame_id.to_string());
        assert_eq!(begin["args"]["present_outcome"], "Presented");
        assert_eq!(
            begin["args"]["presentation"],
            snapshots[0].presentation.to_string(),
            "the exported args must name the producing presentation, not just the frame_id"
        );
        assert!(
            begin["name"]
                .as_str()
                .expect("name must be a string")
                .contains(&snapshots[0].presentation.to_string()),
            "the event NAME must also embed the presentation, so two presentations' frame_id \
             sequences (each starting from 1) cannot collide in one exported trace file"
        );
    }

    /// Two inputs before one recorded frame: both survive the export, with
    /// the older arrival carrying the strictly larger latency — kills
    /// "last-input-wins" attribution surviving all the way to the exported
    /// JSON, not just at the `FrameSnapshot` level.
    #[test]
    fn frame_snapshot_export_preserves_coalescing_order_and_latency_ordering() {
        use flui_scheduler::{
            ClockSource, DemandKind, FrameClock, PollDecision, PresentOutcome, PresentationId,
        };

        let clock = FrameClock::with_source(ClockSource::Platform);
        let older = clock.stamp_input_epoch(clock.now());
        thread::sleep(std::time::Duration::from_millis(5));
        let newer = clock.stamp_input_epoch(clock.now());

        clock.mark_demand(DemandKind::Dirty);
        let now = clock.now();
        assert_eq!(clock.poll(now), PollDecision::Produce);
        let submit_at = clock.now();
        let _ = clock.record_frame(
            PresentationId::new(1),
            now,
            now,
            now,
            submit_at,
            PresentOutcome::Presented,
        );

        let timeline = Timeline::new();
        timeline.record_frame_snapshots(&clock.frames_since(None));
        let parsed: serde_json::Value =
            serde_json::from_str(&timeline.export_chrome_trace()).expect("valid JSON");
        let inputs = parsed["traceEvents"][0]["args"]["inputs"]
            .as_array()
            .expect("inputs array");
        assert_eq!(inputs.len(), 2);

        let latency_of = |id: flui_scheduler::InputEpochId| {
            inputs
                .iter()
                .find(|entry| entry["input_epoch_id"].as_u64() == Some(id.get()))
                .and_then(|entry| entry["latency_us"].as_u64())
                .expect("id must be present with a latency")
        };
        assert!(
            latency_of(older) > latency_of(newer),
            "the older arrival must show the larger latency in the exported JSON"
        );
    }

    /// Two DIFFERENT presentations, each producing their own "frame 1" (a
    /// `FrameSnapshot::frame_id` is scoped to the clock that minted it, so
    /// both start counting from 1) — exported into the SAME `Timeline`, as
    /// `flui-app`'s own realm-wide devtools export would for a realm
    /// hosting more than one presentation. The two events must be
    /// distinguishable by NAME, not merely by an args field a consumer
    /// would have to already know to inspect.
    #[test]
    fn two_presentations_own_colliding_frame_ids_export_as_distinguishable_events() {
        use flui_scheduler::{
            ClockSource, DemandKind, FrameClock, PollDecision, PresentOutcome, PresentationId,
        };

        let record_one = |presentation: PresentationId| -> FrameSnapshot {
            let clock = FrameClock::with_source(ClockSource::Platform);
            clock.mark_demand(DemandKind::Dirty);
            let now = clock.now();
            assert_eq!(clock.poll(now), PollDecision::Produce);
            clock.record_frame(presentation, now, now, now, now, PresentOutcome::Presented)
        };

        let a = record_one(PresentationId::new(1));
        let b = record_one(PresentationId::new(2));
        assert_eq!(
            a.frame_id, b.frame_id,
            "sanity: two independent clocks' first frame_id really do collide"
        );

        let timeline = Timeline::new();
        timeline.record_frame_snapshots(&[a, b]);
        let parsed: serde_json::Value =
            serde_json::from_str(&timeline.export_chrome_trace()).expect("valid JSON");
        let trace_events = parsed["traceEvents"].as_array().expect("array");
        // One Begin + one End per recorded frame, two frames recorded.
        assert_eq!(trace_events.len(), 4);

        let begin_names: std::collections::HashSet<&str> = trace_events
            .iter()
            .filter(|event| event["ph"] == "B")
            .map(|event| event["name"].as_str().expect("name is a string"))
            .collect();
        assert_eq!(
            begin_names.len(),
            2,
            "two colliding frame_ids must still produce two DISTINCT event names once \
             exported into the same trace, not one name overwriting the other conceptually \
             (the trace format itself would keep both events either way, but a consumer \
             reading names alone must be able to tell them apart): got {begin_names:?}"
        );
    }
}
