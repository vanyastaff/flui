# FLUI DevTools Architecture

**Version:** 0.1.0
**Date:** 2025-11-10
**Author:** Claude (Anthropic)
**Status:** Design Proposal

---

## Executive Summary

This document defines the complete architecture for FLUI's developer tools (`flui_devtools`), based on Flutter DevTools. The system provides **comprehensive debugging**, **performance profiling**, and **inspection tools** for FLUI applications.

**Key Design Principles:**
1. **Non-Intrusive**: DevTools connects to running apps via protocol (like Chrome DevTools)
2. **Performance Overlay**: In-app overlay showing FPS, frame times, memory usage
3. **Widget Inspector**: Visual tree inspection with property viewing
4. **Timeline Profiler**: Frame-by-frame performance analysis
5. **Memory Profiler**: Heap snapshots, leak detection
6. **Network Inspector**: HTTP request/response monitoring
7. **Logging**: Structured logging with filtering
8. **Debugger**: Breakpoints, stepping, variable inspection (future)

**Architecture Overview:**
```
┌─────────────────────────────────────────────────────────────┐
│                  DevTools Web App (Tauri)                   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  • Widget Inspector                                  │   │
│  │  • Performance Timeline                              │   │
│  │  • Memory Profiler                                   │   │
│  │  │  Network Inspector                                 │   │
│  │  • Logging Console                                   │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                          ↕ WebSocket
┌─────────────────────────────────────────────────────────────┐
│              DevTools Service Protocol (JSON)               │
│  • Element tree inspection                                  │
│  • Performance events                                       │
│  • Memory snapshots                                         │
│  • Network events                                           │
│  • Log messages                                             │
└─────────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────────┐
│           Running FLUI App (with DevTools enabled)          │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Performance Overlay (optional)                      │   │
│  │  Instrumentation & Event Recording                   │   │
│  │  Element Tree Service                                │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

**Total Work Estimate:** ~4,000 LOC (protocol ~500 + instrumentation ~1,000 + overlay ~800 + web app ~1,700)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [DevTools Service Protocol](#devtools-service-protocol)
3. [Performance Overlay](#performance-overlay)
4. [Widget Inspector](#widget-inspector)
5. [Timeline Profiler](#timeline-profiler)
6. [Memory Profiler](#memory-profiler)
7. [Network Inspector](#network-inspector)
8. [Logging Console](#logging-console)
9. [DevTools Web App](#devtools-web-app)
10. [Implementation Plan](#implementation-plan)
11. [Usage Examples](#usage-examples)
12. [Testing Strategy](#testing-strategy)

---

## Architecture Overview

### Two-Part System

**1. Instrumentation (Embedded in App)**
```rust
// In your app (debug/profile mode only)
#[cfg(debug_assertions)]
{
    flui_devtools::enable();
}

runApp(MyApp::new());
```

**2. DevTools UI (Separate Web App)**
```bash
# Launch DevTools
flui devtools

# Auto-connects to running app on localhost:9100
```

### Communication Flow

```text
FLUI App (Port 9100)
    ↓ WebSocket
DevTools Protocol (JSON messages)
    ↓
DevTools Web App (Tauri/Yew)
    ↓ Display
Inspector/Profiler/Logger UI
```

---

## DevTools Service Protocol

### Protocol Messages (JSON)

```rust
// In flui_devtools/src/protocol/mod.rs

/// DevTools protocol message
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DevToolsMessage {
    // Widget Inspector
    GetElementTree { id: u64 },
    ElementTreeResponse { id: u64, tree: ElementTreeSnapshot },
    SelectElement { element_id: String },
    GetElementProperties { element_id: String },
    ElementPropertiesResponse { element_id: String, properties: HashMap<String, serde_json::Value> },

    // Performance
    StartPerformanceRecording { id: u64 },
    StopPerformanceRecording { id: u64 },
    PerformanceEvent { event: PerformanceEventData },
    GetPerformanceProfile { id: u64 },
    PerformanceProfileResponse { id: u64, profile: PerformanceProfile },

    // Memory
    RequestHeapSnapshot { id: u64 },
    HeapSnapshotResponse { id: u64, snapshot: HeapSnapshot },
    GetMemoryUsage { id: u64 },
    MemoryUsageResponse { id: u64, usage: MemoryUsage },

    // Network
    HttpRequest { request_id: String, request: HttpRequestData },
    HttpResponse { request_id: String, response: HttpResponseData },

    // Logging
    LogMessage { level: LogLevel, message: String, timestamp: u64 },

    // App Info
    GetAppInfo { id: u64 },
    AppInfoResponse { id: u64, info: AppInfo },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ElementTreeSnapshot {
    pub root: ElementNode,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ElementNode {
    pub id: String,
    pub widget_type: String,
    pub key: Option<String>,
    pub size: Option<(f64, f64)>,
    pub offset: Option<(f64, f64)>,
    pub children: Vec<ElementNode>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceEventData {
    pub timestamp: u64,
    pub event_type: PerformanceEventType,
    pub duration_micros: Option<u64>,
    pub data: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PerformanceEventType {
    FrameStart,
    FrameEnd,
    BuildStart,
    BuildEnd,
    LayoutStart,
    LayoutEnd,
    PaintStart,
    PaintEnd,
    CompositingStart,
    CompositingEnd,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceProfile {
    pub frames: Vec<FrameProfile>,
    pub build_times: Vec<u64>,
    pub layout_times: Vec<u64>,
    pub paint_times: Vec<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrameProfile {
    pub frame_number: u64,
    pub timestamp: u64,
    pub build_time_micros: u64,
    pub layout_time_micros: u64,
    pub paint_time_micros: u64,
    pub total_time_micros: u64,
    pub dropped: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeapSnapshot {
    pub timestamp: u64,
    pub total_size: usize,
    pub objects: Vec<HeapObject>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeapObject {
    pub id: String,
    pub type_name: String,
    pub size: usize,
    pub references: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryUsage {
    pub timestamp: u64,
    pub heap_used: usize,
    pub heap_total: usize,
    pub external: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpRequestData {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpResponseData {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timestamp: u64,
    pub duration_millis: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub flui_version: String,
    pub debug_mode: bool,
    pub target: String,
}
```

### WebSocket Server

```rust
// In flui_devtools/src/server.rs

use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;

/// DevTools WebSocket server
pub struct DevToolsServer {
    listener: TcpListener,
    clients: Arc<Mutex<Vec<WebSocketStream>>>,
    event_tx: broadcast::Sender<DevToolsMessage>,
}

impl DevToolsServer {
    pub async fn new(port: u16) -> Result<Self, DevToolsError> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        let (event_tx, _) = broadcast::channel(1000);

        tracing::info!("DevTools server listening on port {}", port);

        Ok(Self {
            listener,
            clients: Arc::new(Mutex::new(Vec::new())),
            event_tx,
        })
    }

    /// Run the server (async)
    pub async fn run(self) -> Result<(), DevToolsError> {
        loop {
            let (stream, addr) = self.listener.accept().await?;
            tracing::info!("DevTools client connected: {}", addr);

            let ws_stream = accept_async(stream).await?;
            self.clients.lock().await.push(ws_stream);

            // Spawn client handler
            let clients = self.clients.clone();
            let event_tx = self.event_tx.clone();
            tokio::spawn(async move {
                Self::handle_client(ws_stream, clients, event_tx).await;
            });
        }
    }

    async fn handle_client(
        mut ws: WebSocketStream,
        clients: Arc<Mutex<Vec<WebSocketStream>>>,
        event_tx: broadcast::Sender<DevToolsMessage>,
    ) {
        let mut event_rx = event_tx.subscribe();

        loop {
            tokio::select! {
                // Incoming message from client
                msg = ws.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(msg) = serde_json::from_str::<DevToolsMessage>(&text) {
                                Self::handle_message(msg, &event_tx).await;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }

                // Outgoing event to client
                event = event_rx.recv() => {
                    if let Ok(event) = event {
                        let json = serde_json::to_string(&event).unwrap();
                        let _ = ws.send(Message::Text(json)).await;
                    }
                }
            }
        }

        // Remove client on disconnect
        clients.lock().await.retain(|c| !Arc::ptr_eq(c, &ws));
    }

    async fn handle_message(msg: DevToolsMessage, event_tx: &broadcast::Sender<DevToolsMessage>) {
        // Dispatch to appropriate handler
        match msg {
            DevToolsMessage::GetElementTree { id } => {
                // Get element tree snapshot
                let tree = DevToolsState::global().get_element_tree();
                let response = DevToolsMessage::ElementTreeResponse { id, tree };
                let _ = event_tx.send(response);
            }
            DevToolsMessage::StartPerformanceRecording { .. } => {
                DevToolsState::global().start_performance_recording();
            }
            _ => {}
        }
    }

    /// Broadcast event to all clients
    pub fn broadcast(&self, event: DevToolsMessage) {
        let _ = self.event_tx.send(event);
    }
}
```

### Global DevTools State

```rust
// In flui_devtools/src/state.rs

/// Global DevTools state
pub struct DevToolsState {
    enabled: AtomicBool,
    performance_recording: AtomicBool,
    performance_events: Arc<Mutex<Vec<PerformanceEventData>>>,
    network_requests: Arc<Mutex<HashMap<String, (HttpRequestData, Option<HttpResponseData>)>>>,
    logs: Arc<Mutex<Vec<LogMessage>>>,
    server: Arc<Mutex<Option<DevToolsServer>>>,
}

impl DevToolsState {
    /// Get global instance
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<DevToolsState> = OnceLock::new();
        INSTANCE.get_or_init(|| Self {
            enabled: AtomicBool::new(false),
            performance_recording: AtomicBool::new(false),
            performance_events: Arc::new(Mutex::new(Vec::new())),
            network_requests: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            server: Arc::new(Mutex::new(None)),
        })
    }

    /// Enable DevTools
    pub fn enable(&self) {
        if self.enabled.swap(true, Ordering::Relaxed) {
            return; // Already enabled
        }

        tracing::info!("Enabling FLUI DevTools");

        // Start WebSocket server
        tokio::spawn(async {
            let server = DevToolsServer::new(9100).await.unwrap();
            *DevToolsState::global().server.lock().await = Some(server);
            server.run().await.unwrap();
        });
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Record performance event
    pub fn record_performance_event(&self, event: PerformanceEventData) {
        if !self.performance_recording.load(Ordering::Relaxed) {
            return;
        }

        self.performance_events.lock().push(event);

        // Broadcast to DevTools clients
        if let Some(server) = self.server.lock().as_ref() {
            server.broadcast(DevToolsMessage::PerformanceEvent { event });
        }
    }

    /// Get element tree snapshot
    pub fn get_element_tree(&self) -> ElementTreeSnapshot {
        // Get element tree from WidgetsBinding
        let binding = WidgetsFlutterBinding::ensure_initialized();
        let tree = binding.widgets().element_tree();

        self.snapshot_element_tree(&tree)
    }

    fn snapshot_element_tree(&self, tree: &ElementTree) -> ElementTreeSnapshot {
        // TODO: Implement tree traversal
        ElementTreeSnapshot {
            root: ElementNode {
                id: "root".to_string(),
                widget_type: "App".to_string(),
                key: None,
                size: None,
                offset: None,
                children: Vec::new(),
            },
        }
    }
}
```

---

## Performance Overlay

### In-App Overlay (Optional)

```rust
// In flui_devtools/src/overlay/mod.rs

/// Performance overlay widget
///
/// Shows FPS, frame times, and memory usage in-app.
#[derive(Debug)]
pub struct PerformanceOverlay {
    show_fps: bool,
    show_frame_times: bool,
    show_memory: bool,
    position: OverlayPosition,
}

#[derive(Debug, Clone, Copy)]
pub enum OverlayPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl PerformanceOverlay {
    pub fn new() -> Self {
        Self {
            show_fps: true,
            show_frame_times: true,
            show_memory: true,
            position: OverlayPosition::TopRight,
        }
    }

    pub fn show_fps(mut self, show: bool) -> Self {
        self.show_fps = show;
        self
    }

    pub fn position(mut self, position: OverlayPosition) -> Self {
        self.position = position;
        self
    }
}

impl View for PerformanceOverlay {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Get performance metrics
        let metrics = use_memo(ctx, |_| {
            Arc::new(PerformanceMetrics::current())
        });

        // Update every frame
        use_effect(ctx, {
            let metrics = metrics.clone();
            move || {
                let scheduler = WidgetsFlutterBinding::ensure_initialized().scheduler();
                scheduler.add_persistent_frame_callback(Arc::new(move |_| {
                    *metrics.lock() = PerformanceMetrics::current();
                }));
                None
            }
        });

        // Build overlay UI
        Positioned::new(
            self.get_position_offset(),
            Box::new(
                Container::new()
                    .padding(EdgeInsets::all(8.0))
                    .decoration(BoxDecoration::new()
                        .color(Color::BLACK.with_opacity(0.7))
                        .border_radius(BorderRadius::all(4.0)))
                    .child(Box::new(
                        Column::new()
                            .main_axis_size(MainAxisSize::Min)
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .children(self.build_metrics(metrics.lock()))
                    ))
            )
        )
    }
}

impl PerformanceOverlay {
    fn build_metrics(&self, metrics: &PerformanceMetrics) -> Vec<AnyElement> {
        let mut children = Vec::new();

        if self.show_fps {
            children.push(Box::new(
                Text::new(format!("FPS: {:.1}", metrics.fps))
                    .style(TextStyle::new()
                        .color(self.get_fps_color(metrics.fps))
                        .font_size(12.0)
                        .font_family("monospace"))
            ));
        }

        if self.show_frame_times {
            children.push(Box::new(
                Text::new(format!("Frame: {:.1}ms", metrics.frame_time_ms))
                    .style(TextStyle::new()
                        .color(self.get_frame_time_color(metrics.frame_time_ms))
                        .font_size(12.0)
                        .font_family("monospace"))
            ));
        }

        if self.show_memory {
            children.push(Box::new(
                Text::new(format!("Mem: {:.1}MB", metrics.memory_mb))
                    .style(TextStyle::new()
                        .color(Color::WHITE)
                        .font_size(12.0)
                        .font_family("monospace"))
            ));
        }

        children
    }

    fn get_fps_color(&self, fps: f64) -> Color {
        if fps >= 55.0 {
            Color::GREEN
        } else if fps >= 30.0 {
            Color::YELLOW
        } else {
            Color::RED
        }
    }

    fn get_frame_time_color(&self, frame_time: f64) -> Color {
        if frame_time <= 16.7 {
            Color::GREEN
        } else if frame_time <= 33.3 {
            Color::YELLOW
        } else {
            Color::RED
        }
    }

    fn get_position_offset(&self) -> Offset {
        match self.position {
            OverlayPosition::TopLeft => Offset::new(8.0, 8.0),
            OverlayPosition::TopRight => Offset::new(-8.0, 8.0), // Positioned handles right
            OverlayPosition::BottomLeft => Offset::new(8.0, -8.0),
            OverlayPosition::BottomRight => Offset::new(-8.0, -8.0),
        }
    }
}

#[derive(Debug, Clone)]
struct PerformanceMetrics {
    fps: f64,
    frame_time_ms: f64,
    memory_mb: f64,
}

impl PerformanceMetrics {
    fn current() -> Self {
        // Get current metrics from DevToolsState
        Self {
            fps: 60.0, // TODO: Calculate from frame times
            frame_time_ms: 16.7,
            memory_mb: 100.0,
        }
    }
}
```

---

## Widget Inspector

### Element Tree Service

```rust
// In flui_devtools/src/inspector/element_tree_service.rs

/// Service for inspecting element tree
pub struct ElementTreeService;

impl ElementTreeService {
    /// Get element tree snapshot
    pub fn get_element_tree() -> ElementTreeSnapshot {
        let binding = WidgetsFlutterBinding::ensure_initialized();
        let tree = binding.widgets().element_tree();

        Self::build_snapshot(tree)
    }

    fn build_snapshot(tree: &ElementTree) -> ElementTreeSnapshot {
        // Get root element
        let root_id = tree.root_element();

        ElementTreeSnapshot {
            root: Self::build_node(tree, root_id),
        }
    }

    fn build_node(tree: &ElementTree, element_id: ElementId) -> ElementNode {
        let element = tree.get(element_id).unwrap();

        ElementNode {
            id: format!("{:?}", element_id),
            widget_type: element.widget_type_name(),
            key: element.key().map(|k| k.to_string()),
            size: element.size().map(|s| (s.width, s.height)),
            offset: element.offset().map(|o| (o.dx, o.dy)),
            children: element
                .children()
                .iter()
                .map(|&child_id| Self::build_node(tree, child_id))
                .collect(),
        }
    }

    /// Get element properties
    pub fn get_element_properties(element_id: &str) -> HashMap<String, serde_json::Value> {
        // Parse element ID
        let id = element_id.parse::<usize>().unwrap();
        let element_id = ElementId::new(id);

        let binding = WidgetsFlutterBinding::ensure_initialized();
        let tree = binding.widgets().element_tree();
        let element = tree.get(element_id).unwrap();

        let mut properties = HashMap::new();

        // Add common properties
        properties.insert("type".to_string(), json!(element.widget_type_name()));

        if let Some(key) = element.key() {
            properties.insert("key".to_string(), json!(key.to_string()));
        }

        if let Some(size) = element.size() {
            properties.insert("size".to_string(), json!({
                "width": size.width,
                "height": size.height,
            }));
        }

        if let Some(offset) = element.offset() {
            properties.insert("offset".to_string(), json!({
                "x": offset.dx,
                "y": offset.dy,
            }));
        }

        // TODO: Add widget-specific properties via diagnostics

        properties
    }
}
```

---

## Timeline Profiler

### Performance Event Recording

```rust
// In flui_devtools/src/profiler/timeline.rs

/// Timeline profiler
pub struct TimelineProfiler {
    events: Arc<Mutex<Vec<TimelineEvent>>>,
    frame_start_time: Arc<Mutex<Option<Instant>>>,
    frame_number: AtomicU64,
}

impl TimelineProfiler {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<TimelineProfiler> = OnceLock::new();
        INSTANCE.get_or_init(|| Self {
            events: Arc::new(Mutex::new(Vec::new())),
            frame_start_time: Arc::new(Mutex::new(None)),
            frame_number: AtomicU64::new(0),
        })
    }

    /// Record frame start
    pub fn record_frame_start(&self) {
        let now = Instant::now();
        *self.frame_start_time.lock() = Some(now);

        let event = TimelineEvent {
            timestamp: now,
            frame_number: self.frame_number.load(Ordering::Relaxed),
            event_type: TimelineEventType::FrameStart,
            duration: None,
        };

        self.record_event(event);
    }

    /// Record frame end
    pub fn record_frame_end(&self) {
        let now = Instant::now();
        let start = self.frame_start_time.lock().take();

        let duration = start.map(|s| now.duration_since(s));

        let event = TimelineEvent {
            timestamp: now,
            frame_number: self.frame_number.fetch_add(1, Ordering::Relaxed),
            event_type: TimelineEventType::FrameEnd,
            duration,
        };

        self.record_event(event);
    }

    /// Record build phase
    pub fn record_build(&self, duration: Duration) {
        let event = TimelineEvent {
            timestamp: Instant::now(),
            frame_number: self.frame_number.load(Ordering::Relaxed),
            event_type: TimelineEventType::Build,
            duration: Some(duration),
        };

        self.record_event(event);
    }

    /// Record layout phase
    pub fn record_layout(&self, duration: Duration) {
        let event = TimelineEvent {
            timestamp: Instant::now(),
            frame_number: self.frame_number.load(Ordering::Relaxed),
            event_type: TimelineEventType::Layout,
            duration: Some(duration),
        };

        self.record_event(event);
    }

    /// Record paint phase
    pub fn record_paint(&self, duration: Duration) {
        let event = TimelineEvent {
            timestamp: Instant::now(),
            frame_number: self.frame_number.load(Ordering::Relaxed),
            event_type: TimelineEventType::Paint,
            duration: Some(duration),
        };

        self.record_event(event);
    }

    fn record_event(&self, event: TimelineEvent) {
        self.events.lock().push(event.clone());

        // Broadcast to DevTools
        if DevToolsState::global().is_enabled() {
            let perf_event = PerformanceEventData {
                timestamp: event.timestamp.elapsed().as_micros() as u64,
                event_type: match event.event_type {
                    TimelineEventType::FrameStart => PerformanceEventType::FrameStart,
                    TimelineEventType::FrameEnd => PerformanceEventType::FrameEnd,
                    TimelineEventType::Build => PerformanceEventType::BuildEnd,
                    TimelineEventType::Layout => PerformanceEventType::LayoutEnd,
                    TimelineEventType::Paint => PerformanceEventType::PaintEnd,
                },
                duration_micros: event.duration.map(|d| d.as_micros() as u64),
                data: HashMap::new(),
            };

            DevToolsState::global().record_performance_event(perf_event);
        }
    }

    /// Get performance profile
    pub fn get_profile(&self) -> PerformanceProfile {
        let events = self.events.lock();

        // Group events by frame
        let mut frames: HashMap<u64, Vec<&TimelineEvent>> = HashMap::new();
        for event in events.iter() {
            frames.entry(event.frame_number).or_default().push(event);
        }

        // Build frame profiles
        let mut frame_profiles = Vec::new();
        for (frame_num, frame_events) in frames {
            let mut build_time = 0;
            let mut layout_time = 0;
            let mut paint_time = 0;

            for event in frame_events {
                match event.event_type {
                    TimelineEventType::Build => {
                        build_time = event.duration.unwrap_or_default().as_micros() as u64;
                    }
                    TimelineEventType::Layout => {
                        layout_time = event.duration.unwrap_or_default().as_micros() as u64;
                    }
                    TimelineEventType::Paint => {
                        paint_time = event.duration.unwrap_or_default().as_micros() as u64;
                    }
                    _ => {}
                }
            }

            let total_time = build_time + layout_time + paint_time;
            let dropped = total_time > 16_667; // > 16.67ms (60fps)

            frame_profiles.push(FrameProfile {
                frame_number: frame_num,
                timestamp: 0, // TODO
                build_time_micros: build_time,
                layout_time_micros: layout_time,
                paint_time_micros: paint_time,
                total_time_micros: total_time,
                dropped,
            });
        }

        PerformanceProfile {
            frames: frame_profiles,
            build_times: Vec::new(),
            layout_times: Vec::new(),
            paint_times: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct TimelineEvent {
    timestamp: Instant,
    frame_number: u64,
    event_type: TimelineEventType,
    duration: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
enum TimelineEventType {
    FrameStart,
    FrameEnd,
    Build,
    Layout,
    Paint,
}
```

### Instrumentation Integration

```rust
// In flui_core/src/pipeline/pipeline_owner.rs

impl PipelineOwner {
    pub fn flush_build(&self) {
        #[cfg(debug_assertions)]
        let start = Instant::now();

        // ... existing build code ...

        #[cfg(debug_assertions)]
        {
            let duration = start.elapsed();
            TimelineProfiler::global().record_build(duration);
        }
    }

    pub fn flush_layout(&self) {
        #[cfg(debug_assertions)]
        let start = Instant::now();

        // ... existing layout code ...

        #[cfg(debug_assertions)]
        {
            let duration = start.elapsed();
            TimelineProfiler::global().record_layout(duration);
        }
    }

    pub fn flush_paint(&self) {
        #[cfg(debug_assertions)]
        let start = Instant::now();

        // ... existing paint code ...

        #[cfg(debug_assertions)]
        {
            let duration = start.elapsed();
            TimelineProfiler::global().record_paint(duration);
        }
    }
}
```

---

## Memory Profiler

### Heap Snapshot

```rust
// In flui_devtools/src/profiler/memory.rs

/// Memory profiler
pub struct MemoryProfiler;

impl MemoryProfiler {
    /// Take heap snapshot
    pub fn take_heap_snapshot() -> HeapSnapshot {
        // Get current memory stats
        let stats = Self::get_memory_stats();

        HeapSnapshot {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            total_size: stats.heap_used,
            objects: Vec::new(), // TODO: Implement object tracking
        }
    }

    /// Get current memory usage
    pub fn get_memory_usage() -> MemoryUsage {
        let stats = Self::get_memory_stats();

        MemoryUsage {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            heap_used: stats.heap_used,
            heap_total: stats.heap_total,
            external: stats.external,
        }
    }

    fn get_memory_stats() -> MemoryStats {
        // Platform-specific memory stats
        #[cfg(target_os = "windows")]
        {
            Self::get_windows_memory_stats()
        }

        #[cfg(target_os = "linux")]
        {
            Self::get_linux_memory_stats()
        }

        #[cfg(target_os = "macos")]
        {
            Self::get_macos_memory_stats()
        }
    }

    #[cfg(target_os = "windows")]
    fn get_windows_memory_stats() -> MemoryStats {
        use windows::Win32::System::ProcessStatus::*;

        unsafe {
            let mut pmc = PROCESS_MEMORY_COUNTERS::default();
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut pmc,
                size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            );

            MemoryStats {
                heap_used: pmc.WorkingSetSize,
                heap_total: pmc.QuotaPeakPagedPoolUsage,
                external: 0,
            }
        }
    }
}

struct MemoryStats {
    heap_used: usize,
    heap_total: usize,
    external: usize,
}
```

---

## Network Inspector

### HTTP Request/Response Tracking

```rust
// In flui_devtools/src/inspector/network.rs

/// Network inspector
pub struct NetworkInspector;

impl NetworkInspector {
    /// Record HTTP request
    pub fn record_request(request_id: String, request: HttpRequestData) {
        if !DevToolsState::global().is_enabled() {
            return;
        }

        DevToolsState::global()
            .network_requests
            .lock()
            .insert(request_id.clone(), (request.clone(), None));

        // Broadcast to DevTools
        if let Some(server) = DevToolsState::global().server.lock().as_ref() {
            server.broadcast(DevToolsMessage::HttpRequest {
                request_id,
                request,
            });
        }
    }

    /// Record HTTP response
    pub fn record_response(request_id: String, response: HttpResponseData) {
        if !DevToolsState::global().is_enabled() {
            return;
        }

        // Update request entry
        if let Some((_, resp)) = DevToolsState::global()
            .network_requests
            .lock()
            .get_mut(&request_id)
        {
            *resp = Some(response.clone());
        }

        // Broadcast to DevTools
        if let Some(server) = DevToolsState::global().server.lock().as_ref() {
            server.broadcast(DevToolsMessage::HttpResponse {
                request_id,
                response,
            });
        }
    }
}

/// Instrumentation for reqwest
#[cfg(feature = "reqwest")]
pub mod reqwest_instrumentation {
    use super::*;
    use reqwest::{Client, Request, Response};

    /// Instrumented reqwest client
    pub struct InstrumentedClient {
        client: Client,
    }

    impl InstrumentedClient {
        pub fn new(client: Client) -> Self {
            Self { client }
        }

        pub async fn execute(&self, request: Request) -> Result<Response, reqwest::Error> {
            let request_id = uuid::Uuid::new_v4().to_string();

            // Record request
            NetworkInspector::record_request(
                request_id.clone(),
                HttpRequestData {
                    method: request.method().to_string(),
                    url: request.url().to_string(),
                    headers: request
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect(),
                    body: request.body().and_then(|b| b.as_bytes().map(|b| b.to_vec())),
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                },
            );

            let start = Instant::now();
            let response = self.client.execute(request).await?;
            let duration = start.elapsed();

            // Record response
            NetworkInspector::record_response(
                request_id,
                HttpResponseData {
                    status: response.status().as_u16(),
                    headers: response
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect(),
                    body: None, // TODO: Capture body
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    duration_millis: duration.as_millis() as u64,
                },
            );

            Ok(response)
        }
    }
}
```

---

## Logging Console

### Structured Logging Integration

```rust
// In flui_devtools/src/logging/mod.rs

/// DevTools logging layer (tracing subscriber)
pub struct DevToolsLoggingLayer;

impl<S> tracing_subscriber::Layer<S> for DevToolsLoggingLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if !DevToolsState::global().is_enabled() {
            return;
        }

        // Extract level
        let level = match *event.metadata().level() {
            tracing::Level::TRACE => LogLevel::Trace,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::ERROR => LogLevel::Error,
        };

        // Format message
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Send to DevTools
        let log_msg = DevToolsMessage::LogMessage {
            level,
            message: visitor.message,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
        };

        if let Some(server) = DevToolsState::global().server.lock().as_ref() {
            server.broadcast(log_msg);
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}

/// Initialize DevTools logging
pub fn init_devtools_logging() {
    use tracing_subscriber::layer::SubscriberExt;

    let subscriber = tracing_subscriber::registry()
        .with(DevToolsLoggingLayer)
        .with(tracing_subscriber::fmt::layer());

    tracing::subscriber::set_global_default(subscriber).ok();
}
```

---

## DevTools Web App

### Tauri + Yew Frontend

```rust
// In flui_devtools_app/src/main.rs

use tauri::Manager;
use yew::prelude::*;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Open DevTools window
            let window = app.get_window("main").unwrap();
            window.set_title("FLUI DevTools").unwrap();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect_to_app,
            disconnect_from_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn connect_to_app(url: String) -> Result<(), String> {
    // Connect WebSocket to running app
    Ok(())
}

#[tauri::command]
async fn disconnect_from_app() -> Result<(), String> {
    Ok(())
}
```

**Yew Components:**

```rust
// Widget Inspector Component
#[function_component]
fn WidgetInspector() -> Html {
    let element_tree = use_state(|| None);

    html! {
        <div class="widget-inspector">
            <div class="tree-view">
                // Render element tree
            </div>
            <div class="properties-panel">
                // Show selected element properties
            </div>
        </div>
    }
}

// Performance Timeline Component
#[function_component]
fn PerformanceTimeline() -> Html {
    let frames = use_state(|| Vec::new());

    html! {
        <div class="performance-timeline">
            <div class="frame-chart">
                // Chart showing frame times
            </div>
            <div class="event-list">
                // List of timeline events
            </div>
        </div>
    }
}

// Main App Component
#[function_component]
fn App() -> Html {
    let active_tab = use_state(|| "inspector");

    html! {
        <div class="devtools-app">
            <header>
                <h1>{ "FLUI DevTools" }</h1>
                <div class="tabs">
                    <button onclick={set_tab("inspector")}>{ "Inspector" }</button>
                    <button onclick={set_tab("performance")}>{ "Performance" }</button>
                    <button onclick={set_tab("memory")}>{ "Memory" }</button>
                    <button onclick={set_tab("network")}>{ "Network" }</button>
                    <button onclick={set_tab("logging")}>{ "Logging" }</button>
                </div>
            </header>
            <main>
                {
                    match *active_tab {
                        "inspector" => html! { <WidgetInspector /> },
                        "performance" => html! { <PerformanceTimeline /> },
                        "memory" => html! { <MemoryProfiler /> },
                        "network" => html! { <NetworkInspector /> },
                        "logging" => html! { <LoggingConsole /> },
                        _ => html! {},
                    }
                }
            </main>
        </div>
    }
}
```

---

## Implementation Plan

### Phase 1: Protocol & Server (~500 LOC)

1. **protocol/mod.rs** (~200 LOC)
   - Protocol message types
   - Serialization/deserialization

2. **server.rs** (~200 LOC)
   - WebSocket server
   - Client management

3. **state.rs** (~100 LOC)
   - Global DevTools state

**Total Phase 1:** ~500 LOC

### Phase 2: Instrumentation (~1,000 LOC)

4. **profiler/timeline.rs** (~300 LOC)
   - Timeline profiler
   - Event recording

5. **profiler/memory.rs** (~200 LOC)
   - Memory profiler
   - Heap snapshots

6. **inspector/element_tree_service.rs** (~300 LOC)
   - Element tree inspection
   - Property extraction

7. **inspector/network.rs** (~200 LOC)
   - Network request tracking
   - HTTP instrumentation

**Total Phase 2:** ~1,000 LOC

### Phase 3: Performance Overlay (~800 LOC)

8. **overlay/mod.rs** (~400 LOC)
   - Performance overlay widget
   - FPS/frame time display

9. **overlay/metrics.rs** (~200 LOC)
   - Performance metrics collection

10. **logging/mod.rs** (~200 LOC)
    - Logging layer
    - tracing integration

**Total Phase 3:** ~800 LOC

### Phase 4: Web App (~1,700 LOC)

11. **devtools_app/src/main.rs** (~100 LOC)
    - Tauri setup

12. **devtools_app/src/components/inspector.rs** (~400 LOC)
    - Widget inspector UI

13. **devtools_app/src/components/performance.rs** (~400 LOC)
    - Performance timeline UI

14. **devtools_app/src/components/memory.rs** (~300 LOC)
    - Memory profiler UI

15. **devtools_app/src/components/network.rs** (~300 LOC)
    - Network inspector UI

16. **devtools_app/src/components/logging.rs** (~200 LOC)
    - Logging console UI

**Total Phase 4:** ~1,700 LOC

---

## Usage Examples

### Example 1: Enable DevTools

```rust
use flui_app::runApp;
use flui_devtools::enable;

fn main() {
    #[cfg(debug_assertions)]
    enable();

    runApp(MyApp::new());
}
```

### Example 2: Performance Overlay

```rust
use flui_devtools::PerformanceOverlay;

#[derive(Debug)]
struct MyApp;

impl View for MyApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Stack::new()
            .children(vec![
                // Main app content
                Box::new(MyHomeView::new()),

                // Performance overlay (debug only)
                #[cfg(debug_assertions)]
                Box::new(PerformanceOverlay::new()
                    .position(OverlayPosition::TopRight)),
            ])
    }
}
```

### Example 3: Custom Instrumentation

```rust
use flui_devtools::TimelineProfiler;

fn expensive_operation() {
    let start = Instant::now();

    // Do work...

    #[cfg(debug_assertions)]
    {
        let duration = start.elapsed();
        TimelineProfiler::global().record_custom_event("expensive_operation", duration);
    }
}
```

### Example 4: Network Instrumentation

```rust
use flui_devtools::reqwest_instrumentation::InstrumentedClient;

async fn fetch_data() -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();

    #[cfg(debug_assertions)]
    let client = InstrumentedClient::new(client);

    let response = client
        .get("https://api.example.com/data")
        .send()
        .await?;

    response.text().await
}
```

---

## Testing Strategy

### Unit Tests

1. **Protocol Serialization:**
   - Test message encoding/decoding
   - Test all message types

2. **Timeline Profiler:**
   - Test event recording
   - Test frame profile generation

3. **Memory Profiler:**
   - Test snapshot creation
   - Test memory usage calculation

### Integration Tests

1. **WebSocket Communication:**
   - Test client connection
   - Test message broadcast
   - Test client disconnection

2. **End-to-End:**
   - Launch app with DevTools enabled
   - Connect DevTools web app
   - Verify data flow

### Performance Tests

1. **Instrumentation Overhead:**
   - Benchmark timeline recording overhead
   - Measure memory profiler impact
   - Test with 10,000 events/sec

---

## Crate Dependencies

```toml
# crates/flui_devtools/Cargo.toml

[package]
name = "flui_devtools"
version = "0.1.0"
edition = "2021"

[dependencies]
flui_core = { path = "../flui_core" }
flui_types = { path = "../flui_types" }
flui_widgets = { path = "../flui_widgets" }
flui_app = { path = "../flui_app" }

# WebSocket
tokio = { version = "1.43", features = ["full"] }
tokio-tungstenite = "0.21"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# UUID
uuid = { version = "1.0", features = ["v4"] }

# Optional: HTTP instrumentation
reqwest = { version = "0.12", optional = true }

# Platform-specific
[target.'cfg(windows)'.dependencies]
windows = { version = "0.52", features = ["Win32_System_ProcessStatus"] }

[features]
default = []
reqwest = ["dep:reqwest"]
```

```toml
# devtools_app/Cargo.toml (Tauri app)

[package]
name = "flui_devtools_app"
version = "0.1.0"
edition = "2021"

[dependencies]
tauri = { version = "1.5", features = ["shell-open"] }
yew = "0.21"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

---

## Open Questions

1. **Debugger Support:**
   - Should we implement full debugger (breakpoints, stepping)?
   - Use DAP (Debug Adapter Protocol)?

2. **Hot Reload:**
   - Should DevTools trigger hot reload?
   - How to preserve state during reload?

3. **Widget Editing:**
   - Should we allow live widget editing in inspector?
   - How to handle widget immutability?

4. **Distributed Tracing:**
   - Should we support distributed tracing (OpenTelemetry)?
   - Integration with external APM tools?

---

## Version History

| Version | Date       | Author | Changes                       |
|---------|------------|--------|-------------------------------|
| 0.1.0   | 2025-11-10 | Claude | Initial DevTools architecture |

---

## References

- [Flutter DevTools](https://docs.flutter.dev/tools/devtools)
- [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
- [Dart VM Service Protocol](https://github.com/dart-lang/sdk/blob/main/runtime/vm/service/service.md)

---

## Conclusion

This architecture provides **comprehensive developer tools** for FLUI:

✅ **Performance overlay** (FPS, frame times, memory)
✅ **Widget inspector** (element tree, properties)
✅ **Timeline profiler** (frame-by-frame analysis)
✅ **Memory profiler** (heap snapshots, usage)
✅ **Network inspector** (HTTP request/response)
✅ **Logging console** (structured logging)
✅ **WebSocket protocol** (non-intrusive connection)
✅ **Tauri web app** (native desktop UI)

**Key Patterns:**
1. **Non-Intrusive**: WebSocket protocol, zero overhead when disabled
2. **Conditional Compilation**: Instrumentation only in debug/profile builds
3. **Broadcast Pattern**: Single server, multiple clients
4. **Timeline Events**: Frame-by-frame performance tracking

**Estimated Total Work:** ~4,000 LOC
- Protocol & server (~500 LOC)
- Instrumentation (~1,000 LOC)
- Performance overlay (~800 LOC)
- Web app (~1,700 LOC)

This provides production-ready developer tools for FLUI! 🔧📊
