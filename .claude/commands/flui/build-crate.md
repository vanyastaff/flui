---
name: Build Crate
description: Build a specific FLUI crate with its dependencies in correct order
---

Build the specified FLUI crate: **$ARGUMENTS**

Follow these steps:
1. Identify the crate layer (Foundation, Framework, Rendering, Widget, Application)
2. Build dependencies first according to CLAUDE.md dependency order
3. Run `cargo build -p <crate_name>` for the target crate
4. Report any errors with file locations

**Layer order:**
- Foundation: flui_types, flui-foundation, flui-tree
- Framework: flui-view, flui-pipeline, flui-reactivity, flui-scheduler, flui_core
- Rendering: flui_painting, flui_engine, flui_rendering
- Widget/App: flui_widgets, flui_app

If no crate specified, build the entire workspace with `cargo build --workspace`.
