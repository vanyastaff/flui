# FLUI DevTools Architecture

**Version:** 0.1.0
**Date:** 2025-01-10
**Status:** 📋 Design Proposal

---

## Executive Summary

This document provides a high-level architecture overview for FLUI's developer tools (`flui_devtools`). For detailed specifications, see the linked documents below.

**Purpose:** Provide comprehensive debugging, profiling, and inspection tools for FLUI applications

**Key Components:**
1. **Instrumentation** - Embedded in app (debug/profile builds)
2. **Protocol** - WebSocket-based communication (JSON messages)
3. **Web UI** - Standalone DevTools application (Tauri + Yew)
4. **In-App Overlay** - Optional FPS/memory overlay (like Flutter DevTools)

**Architecture Pattern:** Client-Server (DevTools UI ↔ Running App)

---

## Architecture Overview

### System Components

```text
┌─────────────────────────────────────────────────────────────┐
│              DevTools Web App (Tauri + Yew)                 │
│                                                              │
│  UI Components:                                              │
│  • Widget Inspector    - Element tree visualization         │
│  • Timeline Profiler   - Frame performance analysis          │
│  • Memory Profiler     - Heap snapshots & leak detection    │
│  • Network Inspector   - HTTP request monitoring            │
│  • Logging Console     - Structured log viewing             │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ↓ WebSocket (JSON Protocol)
                         │
┌────────────────────────┴────────────────────────────────────┐
│            DevTools Service (in flui_devtools)              │
│                                                              │
│  Protocol Layer:                                             │
│  • Message serialization/deserialization                     │
│  • Connection management                                     │
│  • Event streaming                                           │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ↓ Instrumentation API
                         │
┌────────────────────────┴────────────────────────────────────┐
│              Running FLUI Application                        │
│                                                              │
│  Instrumentation:                                            │
│  • Element tree hooks                                        │
│  • Performance measurement                                   │
│  • Memory tracking                                           │
│  • Network interception                                      │
│  • Optional in-app overlay                                   │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Principles

1. **Non-Intrusive**
   - DevTools connects to running apps via protocol
   - No source code modification required
   - Enable with simple feature flag

2. **Zero Performance Impact (Production)**
   - Instrumentation only in debug/profile builds
   - Conditional compilation (`#[cfg(debug_assertions)]`)
   - No overhead in release builds

3. **Protocol-Based**
   - Language-agnostic JSON protocol
   - WebSocket for real-time updates
   - Versioned API for compatibility

4. **Modular Design**
   - Each tool (inspector, profiler, etc.) is independent
   - Can be enabled/disabled separately
   - Pluggable architecture for extensions

---

## Core Subsystems

### 1. Protocol Layer

**Purpose:** Communication between app and DevTools UI

**Key Features:**
- WebSocket-based (port 9100 default)
- JSON message format
- Request/response + event streaming
- Versioned protocol (v1.0)

**See:** [Protocol Specification](devtools/PROTOCOL.md) (to be created)

### 2. Instrumentation

**Purpose:** Collect runtime data from FLUI app

**Key Features:**
- Element tree introspection
- Frame timing measurement
- Memory allocation tracking
- Network request interception
- Log message capture

**See:** [Instrumentation Guide](devtools/INSTRUMENTATION.md) (to be created)

### 3. Web UI

**Purpose:** Visual interface for developers

**Technology Stack:**
- **Tauri** - Desktop app framework
- **Yew** - Rust frontend framework
- **WebSocket client** - Protocol communication

**See:** [Web UI Architecture](devtools/WEB_UI.md) (to be created)

### 4. Performance Overlay

**Purpose:** In-app FPS/memory display

**Key Features:**
- Minimal overhead (<1% CPU)
- Toggleable with keyboard shortcut
- Shows FPS, frame times, memory usage
- Rendered as top-level overlay

**See:** [Overlay Design](devtools/OVERLAY.md) (to be created)

---

## Integration Points

### With flui_core

**Element Tree Access:**
- Read-only access to ElementTree
- Traverse element hierarchy
- Query element properties

**Location:** `crates/flui_core/src/element/element_tree.rs`

### With flui_engine

**Performance Metrics:**
- Frame timing (build/layout/paint)
- GPU render time
- Layer composition stats

**Location:** `crates/flui_engine/src/compositor.rs`

### With flui_app

**Lifecycle Hooks:**
- App start/stop events
- Hot reload triggers
- Error/exception capture

**Location:** `crates/flui_app/src/binding.rs`

---

## Usage

### Enabling DevTools in App

```rust
// In main.rs (debug/profile builds only)
#[cfg(any(debug_assertions, feature = "profile"))]
{
    flui_devtools::init()
        .enable_overlay()      // Optional: Show FPS overlay
        .enable_inspector()    // Enable widget inspector
        .enable_profiler()     // Enable performance profiling
        .start();
}

// Run app normally
flui::runApp(MyApp::new());
```

### Launching DevTools UI

```bash
# Method 1: Via CLI
flui devtools

# Method 2: Direct execution
flui-devtools-ui
```

### Connecting to Running App

DevTools UI auto-discovers apps on `localhost:9100`. Manual connection also supported.

---

## Implementation Status

| Component | Status | Progress |
|-----------|--------|----------|
| **Protocol Spec** | 📋 Design | Documented below |
| **Instrumentation** | 📋 Design | Not started |
| **Web UI** | 📋 Design | Not started |
| **Overlay** | 📋 Design | Not started |
| **Widget Inspector** | 📋 Design | Not started |
| **Timeline Profiler** | 📋 Design | Not started |
| **Memory Profiler** | 📋 Design | Not started |
| **Network Inspector** | 📋 Design | Not started |

**Overall:** 0% implemented (design phase)

---

## Implementation Roadmap

### Phase 1: Foundation (4-6 weeks)
- Protocol specification
- Basic instrumentation
- WebSocket server
- Simple web UI (connection only)

### Phase 2: Core Tools (6-8 weeks)
- Widget Inspector
- Performance Overlay
- Basic timeline profiler

### Phase 3: Advanced Tools (8-10 weeks)
- Memory profiler
- Network inspector
- Logging console

### Phase 4: Polish (2-4 weeks)
- Hot reload integration
- Error recovery
- Documentation
- Examples

**Total Estimated Time:** 20-28 weeks (5-7 months)

---

## Related Documentation

### Detailed Specifications
- **Protocol**: [devtools/PROTOCOL.md](devtools/PROTOCOL.md) - WebSocket protocol specification
- **Instrumentation**: [devtools/INSTRUMENTATION.md](devtools/INSTRUMENTATION.md) - How to instrument FLUI apps
- **Web UI**: [devtools/WEB_UI.md](devtools/WEB_UI.md) - Tauri + Yew architecture
- **Overlay**: [devtools/OVERLAY.md](devtools/OVERLAY.md) - In-app overlay design

### Integration
- **Integration Guide**: [../INTEGRATION.md](../INTEGRATION.md) - How DevTools fits into FLUI
- **Patterns**: [../PATTERNS.md](../PATTERNS.md) - Common patterns used

### Related Architecture
- **flui_core**: [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md) - Element tree access
- **flui_engine**: [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md) - Performance metrics
- **flui_app**: [APP_ARCHITECTURE.md](APP_ARCHITECTURE.md) - Lifecycle integration

### External References
- **Flutter DevTools**: [flutter.dev/devtools](https://docs.flutter.dev/development/tools/devtools/overview)
- **Chrome DevTools Protocol**: [chromedevtools.github.io](https://chromedevtools.github.io/devtools-protocol/)
- **Tauri**: [tauri.app](https://tauri.app/) - Desktop app framework
- **Yew**: [yew.rs](https://yew.rs/) - Rust frontend framework

---

## Summary

FLUI DevTools provides comprehensive debugging and profiling for FLUI applications:

- ✅ **Non-intrusive** - Protocol-based, no source changes needed
- ✅ **Zero production overhead** - Only enabled in debug/profile builds
- ✅ **Rich tooling** - Inspector, profiler, memory, network, logging
- ✅ **Modern stack** - Tauri + Yew for cross-platform UI

**Current Status:** Design phase - detailed specifications in progress

**Next Steps:**
1. Finalize protocol specification → [devtools/PROTOCOL.md](devtools/PROTOCOL.md)
2. Implement basic instrumentation
3. Build minimal web UI
4. Iterate based on feedback

For implementation details, see the linked specification documents.
