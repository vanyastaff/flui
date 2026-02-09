# CLI Command Contracts: flui-cli Completion

**Branch**: `001-cli-completion` | **Date**: 2026-02-08

This document defines the input/output contracts for each modified CLI command.

---

## 1. `flui emulators` (rewrite)

### `flui emulators list`

**Input**: None (optional `--platform android|ios` filter)
**Output**: Table of available emulators

```
 flui emulators 

  Platform   Name                          Version    Status
  Android    Pixel_6_API_33                API 33     Running
  Android    Pixel_7_API_34                API 34     Stopped
  iOS        iPhone 15 Pro                 iOS 17.0   Shutdown
  iOS        iPhone SE (3rd generation)    iOS 17.0   Shutdown

  4 emulators found
```

**Exit codes**:
- 0: Success (even if 0 emulators found)
- 1: SDK not found (with installation guidance)

### `flui emulators launch <name>`

**Input**: Emulator name (positional argument)
**Output**: Launch status message

```
 Launching: Pixel_6_API_33 

  ◇ Starting Android emulator...
  ◇ Emulator launched successfully

  Emulator running on emulator-5554
```

**Exit codes**:
- 0: Emulator launched
- 1: Emulator not found (lists available names)
- 1: Launch failed (shows error details)

### CLI argument structure (new)

```
flui emulators <SUBCOMMAND>

Subcommands:
  list     List available emulators and simulators
  launch   Launch a specific emulator or simulator
```

---

## 2. `flui platform add <platforms...>` (implement)

**Input**: One or more platform names (positional)
**Output**: Scaffolding confirmation

```
 flui platform add 

  ◇ Creating platforms/android/ directory
  ◇ Generated AndroidManifest.xml
  ◇ Updated flui.toml: added "android" to target_platforms

  Platform android added successfully
```

**Exit codes**:
- 0: Platform added
- 1: Not a FLUI project (no flui.toml)
- 1: Platform already added (informational, non-destructive)
- 1: Invalid platform name (lists valid names)

**Validation**: Platform name must be one of: `android`, `ios`, `web`, `windows`, `linux`, `macos`

---

## 3. `flui platform remove <platform>` (implement)

**Input**: Platform name (positional)
**Output**: Removal confirmation with prompt

```
 flui platform remove 

  ◆ Remove android platform? This will delete platforms/android/
  │  Yes

  ◇ Removed platforms/android/
  ◇ Updated flui.toml: removed "android" from target_platforms

  Platform android removed
```

**Exit codes**:
- 0: Platform removed
- 0: User cancelled removal
- 1: Platform not configured
- 1: Not a FLUI project

---

## 4. `flui devtools` (implement)

### When flui-devtools is available (feature = "devtools")

**Input**: `--port <PORT>` (default: 9100)
**Output**: Server start message

```
 flui devtools 

  DevTools server started on http://localhost:9100
  Press Ctrl+C to stop
```

### When flui-devtools is not available

**Input**: `--port <PORT>` (default: 9100)
**Output**: Instructions

```
 flui devtools 

  DevTools is not available in this build.

  To enable DevTools, rebuild flui-cli with the devtools feature:
    cargo install flui-cli --features devtools

  DevTools requires the flui-devtools crate.
```

**Exit codes**:
- 0: Server started (blocks until Ctrl+C)
- 1: Port already in use
- 0: Feature not available (informational exit)

---

## 5. `flui create` template changes

### Default mode (crates.io)

Generated `Cargo.toml` excerpt:
```toml
# FLUI Template v0.1.0

[dependencies]
flui_app = "0.1"
flui_widgets = "0.1"
flui_core = "0.1"
flui_types = "0.1"
```

### Local mode (`--local`)

Generated `Cargo.toml` excerpt:
```toml
# FLUI Template v0.1.0 (local development)

[dependencies]
flui_app = { path = "../../crates/flui_app" }
flui_widgets = { path = "../../crates/flui_widgets" }
flui_core = { path = "../../crates/flui_core" }
flui_types = { path = "../../crates/flui_types" }
```

### Platform scaffolding during create

When `--platforms` is provided (e.g., `flui create my-app --platforms android,web`), or from the default `target_platforms` in generated `flui.toml`, the template MUST scaffold the corresponding `platforms/` directories. This reuses the same scaffolding logic as `flui platform add`.

Generated project structure (with `--platforms android,web`):
```
my-app/
├── Cargo.toml
├── flui.toml              # target_platforms = ["android", "web"]
├── src/main.rs
├── assets/
├── platforms/
│   ├── android/            # Scaffolded by platform add logic
│   │   └── AndroidManifest.xml
│   └── web/
│       └── index.html
├── .gitignore              # Already references platforms/* paths
└── README.md
```

For desktop-only defaults (`windows`, `linux`, `macos`), the `platforms/` directory is created but only contains empty marker directories (desktop builds don't need extra scaffolding).

**Note**: The `.gitignore` template already references `platforms/android/`, `platforms/web/`, `platforms/ios/` — confirming this was the intended design.

---

## 6. `flui run --hot-reload` (implement)

**Input**: `--hot-reload` flag (already exists, default true)
**Output**: Watch mode with rebuild cycle

```
 flui run 

  ◇ Building project...
  ◇ Application started (PID 12345)
  ◇ Watching src/ for changes...

  [app output here]

  ◇ Change detected: src/main.rs
  ◇ Rebuilding...
  ◇ Build successful, restarting application...
  ◇ Application restarted (PID 12346)
```

**On build failure**:
```
  ✖ Build failed:
    error[E0308]: mismatched types
    ...
  ◇ Watching for changes... (fix errors and save to retry)
```

**Exit codes**:
- 0: User terminated with Ctrl+C
- 1: Initial build failed (no watch mode entered)
