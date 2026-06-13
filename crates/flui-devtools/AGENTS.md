# AGENTS.md — flui-devtools

Developer tools: profiling, timeline, hot-reload, network monitoring, memory profiling.

**Status:** Not in workspace `default-members`. Build explicitly with `cargo build -p flui-devtools`.

## What lives here

- **Performance profiler** — frame timing, jank detection, build/layout/paint phase profiling
- **Timeline view** — event timeline with frame boundaries and custom trace events
- **Hot reload** (feature: `hot-reload`) — file watching via `notify` crate, automatic rebuilds
- **Network monitor** (feature: `network-monitor`) — HTTP request tracking (stub, deps not yet added)
- **Memory profiler** (feature: `memory-profiler`) — heap analysis (stub, deps not yet added)

## Key constraints

- **Features** — `profiling` (default off), `timeline` (default off), `hot-reload` (gates `notify`), `full` enables all
- **Windows-specific** — `windows-sys` for process memory info (`Win32_System_ProcessStatus`, `Win32_System_Threading`)
- **Serialization** — `serde` + `serde_json` for devtools protocol
- **Timing** — `web-time` (maintained replacement for `instant` crate)
- **Many features are stubs** — `network-monitor`, `memory-profiler`, `remote-debug`, `tracing-support` have commented-out deps
