//! Timeline event tracking for FLUI applications
//!
//! Records and visualizes events over time for performance analysis and debugging.
//! Supports exporting to Chrome DevTools trace format for advanced visualization.
//!
//! # Example
//!
//! ```rust
//! use flui_devtools::timeline::{Timeline, EventCategory};
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

use instant::{Duration, Instant};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

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

/// A single timeline event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Event name/description
    pub name: String,
    /// Start time (microseconds since timeline creation)
    pub start_micros: u128,
    /// Duration (microseconds)
    pub duration_micros: u128,
    /// Event category
    pub category: EventCategory,
    /// Thread ID (for multi-threaded applications)
    #[serde(skip)]
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
            thread_id: std::thread::current().id(),
        };

        // Add event and return its index
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
    /// Returns a guard that automatically records the event duration when dropped.
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
    /// Use this for events that happen at a point in time rather than over a duration.
    pub fn record_instant(&self, name: impl Into<String>, category: EventCategory) {
        let mut inner = self.inner.lock();
        let event_index = inner.start_event(name.into(), category);
        inner.end_event(event_index, Duration::ZERO);
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
    /// Returns a JSON string that can be loaded in chrome://tracing for visualization.
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
                        "args": {
                            "category": event.category.name(),
                        }
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
    /// This is a simpler format than Chrome trace, useful for custom visualization.
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
            by_category
                .entry(event.category)
                .or_insert_with(Vec::new)
                .push(event);
        }

        for (category, category_events) in by_category {
            let total_ms: f64 = category_events.iter().map(|e| e.duration_ms()).sum();
            let avg_ms = total_ms / category_events.len() as f64;

            println!("\n{} ({} events):", category.name(), category_events.len());
            println!("  Total: {:.2}ms", total_ms);
            println!("  Average: {:.2}ms", avg_ms);

            // Show longest events
            let mut sorted = category_events.clone();
            sorted.sort_by(|a, b| b.duration_micros.cmp(&a.duration_micros));

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
    use super::*;
    use std::thread;

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
            timeline.record_instant(format!("Event {}", i), EventCategory::Custom);
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
}
