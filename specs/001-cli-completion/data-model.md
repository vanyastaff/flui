# Data Model: flui-cli Completion

**Branch**: `001-cli-completion` | **Date**: 2026-02-08

## Entities

### Emulator

Represents a virtual device for mobile app testing. Not persisted — queried on-demand from platform SDKs.

| Field | Type | Description |
|-------|------|-------------|
| name | String | Human-readable name (AVD name or simulator name) |
| platform | EmulatorPlatform | Android or iOS |
| id | String | Unique identifier (AVD name for Android, UDID for iOS) |
| status | EmulatorStatus | Running or Stopped |
| target_version | String | API level (Android) or OS version (iOS) |
| device_type | Option\<String\> | Device model (e.g., "iPhone 15 Pro", "Pixel_6") |

**States**: `Stopped` → `Running` (via launch) → `Stopped` (via shutdown or manual close)

**Uniqueness**: `id` field is unique per platform. Android uses AVD name, iOS uses UDID.

### EmulatorPlatform

Enum distinguishing Android emulators from iOS simulators.

| Variant | Notes |
|---------|-------|
| Android | Requires Android SDK (`emulator`, `adb`) |
| Ios | Requires Xcode (`xcrun simctl`), macOS only |

### EmulatorStatus

| Variant | Notes |
|---------|-------|
| Running | Emulator/simulator is booted and responsive |
| Stopped | Emulator/simulator is shut down |

### PlatformScaffold

Represents the directory structure and configuration created when adding platform support to a project.

| Field | Type | Description |
|-------|------|-------------|
| platform | String | Platform name (android, ios, web, windows, linux, macos) |
| directory | PathBuf | Relative path to platform directory (e.g., `platforms/android/`) |
| config_files | Vec\<ScaffoldFile\> | Files to generate in the platform directory |

### ScaffoldFile

| Field | Type | Description |
|-------|------|-------------|
| relative_path | String | Path within the platform directory |
| content | String | Generated file content |

### FluiConfig (existing, modified)

Already exists in `config.rs`. Relevant field for platform management:

| Field | Type | Modification |
|-------|------|-------------|
| build.target_platforms | Vec\<String\> | Read/write during `platform add/remove` |

### ProjectTemplate (existing, modified)

Already exists as `Template` enum in `main.rs`. Relevant changes:

| Field | Type | Modification |
|-------|------|-------------|
| dependency_mode | DependencyMode | New: controls crates.io vs path dependencies |
| flui_version | String | New: version marker from `CARGO_PKG_VERSION` |

### DependencyMode

| Variant | Description |
|---------|-------------|
| CratesIo | Use version specifiers (default) |
| Local | Use path dependencies (`--local` flag) |

## Relationships

```
FluiConfig 1──* PlatformScaffold    (target_platforms lists platforms)
ProjectTemplate 1──1 DependencyMode (templates have one dependency strategy)
Emulator *──1 EmulatorPlatform      (emulators belong to a platform)
```

## Validation Rules

- **Emulator name**: Non-empty, must match an existing AVD/simulator
- **Platform name**: Must be one of: android, ios, web, windows, linux, macos
- **DependencyMode::Local**: Only valid when run from within or adjacent to a FLUI workspace
- **Platform add**: Must not duplicate existing entry in `target_platforms`
- **Platform remove**: Must exist in `target_platforms`, requires confirmation
