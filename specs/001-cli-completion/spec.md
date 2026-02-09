# Feature Specification: flui-cli Completion

**Feature Branch**: `001-cli-completion`  
**Created**: 2026-02-08  
**Status**: Draft  
**Input**: User description: "Complete flui-cli stub commands, templates, and integration tests"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Emulator and Simulator Management (Priority: P1)

A developer working on a cross-platform FLUI app wants to manage Android emulators and iOS simulators directly from the CLI without switching to platform-specific tools. They run `flui emulators list` to see available emulators, then `flui emulators launch Pixel_7` to start an Android emulator for testing.

**Why this priority**: Mobile development is a primary use case for FLUI. Developers currently must use raw `emulator` and `xcrun simctl` commands, creating friction and requiring platform-specific knowledge. This is the most impactful stub command to complete.

**Independent Test**: Can be fully tested by listing, launching, and stopping emulators on a machine with Android SDK and/or Xcode installed. Delivers immediate value by unifying emulator management under a single CLI.

**Acceptance Scenarios**:

1. **Given** Android SDK is installed with AVDs configured, **When** the user runs `flui emulators list`, **Then** all available AVDs are displayed with their names, target API levels, and current status (running/stopped)
2. **Given** Xcode is installed on macOS, **When** the user runs `flui emulators list`, **Then** all available iOS simulators are displayed with their names, device types, OS versions, and status
3. **Given** an AVD named "Pixel_7" exists, **When** the user runs `flui emulators launch Pixel_7`, **Then** the emulator starts and the CLI reports successful launch
4. **Given** no Android SDK is installed, **When** the user runs `flui emulators list`, **Then** the CLI displays a helpful message indicating Android SDK is not found and suggests installation steps
5. **Given** user is on Windows/Linux, **When** they run any iOS simulator command, **Then** the CLI displays a message that iOS simulators are only available on macOS

---

### User Story 2 - Platform Scaffolding (Priority: P1)

A developer starting a new FLUI project wants to add Android support. They run `flui platform add android` and the CLI scaffolds the necessary platform-specific directory structure, configuration files, and updates the project's `flui.toml` to reflect the new target platform.

**Why this priority**: Platform scaffolding is essential for multi-platform development. Without it, developers must manually create directory structures and configuration files, which is error-prone and undocumented.

**Independent Test**: Can be tested by creating a FLUI project, adding a platform, verifying the directory structure was created, and confirming `flui.toml` was updated.

**Acceptance Scenarios**:

1. **Given** a valid FLUI project, **When** the user runs `flui platform add android`, **Then** a `platforms/android/` directory is created with appropriate scaffolding and `flui.toml` is updated to include "android" in `target_platforms`
2. **Given** a valid FLUI project with Android already added, **When** the user runs `flui platform add android`, **Then** the CLI informs the user that Android support is already configured
3. **Given** a valid FLUI project with Android support, **When** the user runs `flui platform remove android`, **Then** the `platforms/android/` directory is removed and `flui.toml` is updated to remove "android" from `target_platforms`, with a confirmation prompt before deletion
4. **Given** the user is not in a FLUI project directory, **When** they run `flui platform add android`, **Then** the CLI displays an error that no FLUI project was found
5. **Given** a valid FLUI project, **When** the user runs `flui platform add ios` on a non-macOS system, **Then** the CLI adds iOS configuration but warns that building for iOS requires macOS

---

### User Story 3 - Template Dependency Resolution (Priority: P2)

A developer runs `flui create my-app` to scaffold a new project. The generated `Cargo.toml` references FLUI crates with correct dependency declarations that work regardless of whether the project is inside or outside the FLUI workspace.

**Why this priority**: Templates are the first thing new users interact with. If generated projects don't compile, it creates a terrible first impression and blocks adoption. This is critical for usability but less complex than the stub commands.

**Independent Test**: Can be tested by creating a project with each template and running `cargo check` on the generated project. The project should resolve dependencies correctly.

**Acceptance Scenarios**:

1. **Given** FLUI is published to crates.io, **When** the user runs `flui create my-app`, **Then** the generated `Cargo.toml` references FLUI crates via crates.io version specifiers
2. **Given** the user is developing against a local FLUI checkout, **When** the user runs `flui create my-app --local`, **Then** the generated `Cargo.toml` references FLUI crates via path dependencies relative to the FLUI workspace
3. **Given** the user selects the "counter" template, **When** the project is generated, **Then** all referenced FLUI APIs (signals, widgets, app) exist in the target crate versions
4. **Given** the user creates a project, **When** they inspect the generated files, **Then** a version marker is present indicating which FLUI version the template targets
5. **Given** the user runs `flui create my-app --platforms android,web`, **When** the project is generated, **Then** the `platforms/android/` and `platforms/web/` directories are scaffolded with platform-specific files, and `flui.toml` lists both platforms in `target_platforms`
6. **Given** the user runs `flui create my-app` with default settings, **When** the project is generated, **Then** the `platforms/` directory is created with marker directories for the default desktop platforms listed in `flui.toml`

---

### User Story 4 - Integration Test Suite (Priority: P2)

The flui-cli crate has comprehensive integration tests that verify all commands work correctly end-to-end. A contributor can run `cargo test -p flui-cli` and have confidence that the CLI behaves as expected across all commands.

**Why this priority**: The crate already has `assert_cmd` as a dev dependency but zero integration tests. Without tests, regressions are invisible and refactoring is risky.

**Independent Test**: Can be tested by running `cargo test -p flui-cli` and verifying all tests pass. Each test is self-contained and uses temporary directories.

**Acceptance Scenarios**:

1. **Given** the CLI binary is built, **When** integration tests run `flui create test-project`, **Then** the test verifies a valid project structure was created in a temporary directory
2. **Given** a test project exists, **When** integration tests run `flui doctor`, **Then** the output includes checks for required tooling
3. **Given** an invalid project name (e.g., Rust keyword "fn"), **When** integration tests run `flui create fn`, **Then** the CLI exits with a non-zero code and displays a validation error
4. **Given** the CLI binary is built, **When** integration tests run `flui completions bash`, **Then** valid shell completion script is generated on stdout
5. **Given** integration tests run, **When** all commands are exercised, **Then** test coverage for the CLI crate meets or exceeds 70%

---

### User Story 5 - DevTools Launcher (Priority: P3)

A developer debugging a running FLUI application wants to launch the visual DevTools inspector. They run `flui devtools` and the CLI starts a DevTools server on a configurable port, connecting to their running application for widget inspection and performance profiling.

**Why this priority**: DevTools depends on the `flui-devtools` crate, which is currently disabled in the workspace. This is a lower priority because it cannot be fully implemented until the devtools crate is re-enabled.

**Independent Test**: Can be tested by launching the DevTools server and verifying it binds to the configured port and responds to health checks.

**Acceptance Scenarios**:

1. **Given** `flui-devtools` is available, **When** the user runs `flui devtools`, **Then** a DevTools server starts on the default port (9100) and the CLI reports the URL to connect
2. **Given** port 9100 is in use, **When** the user runs `flui devtools --port 9200`, **Then** the DevTools server starts on port 9200
3. **Given** `flui-devtools` is not installed or not available, **When** the user runs `flui devtools`, **Then** the CLI displays a clear error with instructions on how to enable DevTools

---

### User Story 6 - Hot Reload (Priority: P3)

A developer working on their FLUI app wants fast iteration cycles. They run `flui run --hot-reload` and the CLI watches for file changes, automatically rebuilding and refreshing the running application when source files change.

**Why this priority**: Hot reload is a productivity feature that depends on deeper framework integration (file watching, runtime reload protocol). It requires coordination between the CLI, build system, and application runtime. Important for developer experience but can be deferred.

**Independent Test**: Can be tested by starting a FLUI app with hot reload, modifying a source file, and verifying the application rebuilds and reflects the change.

**Acceptance Scenarios**:

1. **Given** a running FLUI application started with `flui run --hot-reload`, **When** the user modifies a source file and saves it, **Then** the application automatically rebuilds and restarts within a reasonable time
2. **Given** hot reload is active, **When** a compilation error occurs, **Then** the CLI displays the error without crashing the watch process, and resumes watching for changes
3. **Given** the user starts with `flui run` (without `--hot-reload`), **Then** the application runs normally without file watching

---

### Edge Cases

- What happens when the user runs `flui emulators launch` with an emulator name that doesn't exist? The CLI should display an error listing available emulators.
- What happens when `flui platform remove` is run on the last remaining platform? The CLI should warn the user and require explicit confirmation.
- What happens when `flui create` is interrupted mid-generation (e.g., Ctrl+C)? Partially created directories should be cleaned up.
- What happens when multiple emulators are already running and the user launches another? The CLI should proceed but inform the user about already-running instances.
- What happens when `flui platform add` is run for an unsupported platform name (e.g., "playstation")? The CLI should list valid platform names.

## Requirements *(mandatory)*

### Functional Requirements

**Emulators Command:**
- **FR-001**: CLI MUST list available Android AVDs with name, API level, and running status when Android SDK is installed
- **FR-002**: CLI MUST list available iOS simulators with name, device type, OS version, and status when Xcode is installed (macOS only)
- **FR-003**: CLI MUST launch a named Android emulator and report success/failure
- **FR-004**: CLI MUST launch a named iOS simulator and report success/failure (macOS only)
- **FR-005**: CLI MUST display helpful error messages with installation guidance when platform SDKs are not found

**Platform Add/Remove:**
- **FR-006**: CLI MUST scaffold platform-specific directories when adding a platform (e.g., `platforms/android/`, `platforms/ios/`, `platforms/web/`)
- **FR-007**: CLI MUST update `flui.toml` target_platforms list when adding or removing a platform
- **FR-008**: CLI MUST prompt for confirmation before removing a platform's directory and configuration
- **FR-009**: CLI MUST detect and prevent duplicate platform additions

**Templates:**
- **FR-010**: Generated projects MUST use crates.io version specifiers by default for FLUI dependencies
- **FR-011**: Generated projects MUST support a local development mode (`--local` flag) using workspace-relative path dependencies
- **FR-012**: Generated templates MUST include a version marker indicating the FLUI version they target
- **FR-020**: `flui create` MUST scaffold `platforms/` directories for all platforms specified via `--platforms` flag or listed in the default `target_platforms` of `flui.toml`, reusing the same scaffolding logic as `flui platform add`

**Integration Tests:**
- **FR-013**: CLI MUST have integration tests covering project creation with both templates (basic and counter)
- **FR-014**: CLI MUST have integration tests for error cases (invalid names, non-project directories)
- **FR-015**: CLI MUST have integration tests for `doctor`, `completions`, and `platform list` commands

**DevTools:**
- **FR-016**: CLI MUST launch a DevTools server on a configurable port when the devtools crate is available
- **FR-017**: CLI MUST display a clear error with guidance when DevTools dependencies are unavailable

**Hot Reload:**
- **FR-018**: CLI MUST support a `--hot-reload` flag on the `run` command that enables file watching and automatic rebuild
- **FR-019**: CLI MUST handle compilation errors during hot reload gracefully without terminating the watch process

### Key Entities

- **Emulator**: A virtual device for mobile app testing; has a name, platform (Android/iOS), target version, and running status
- **Platform Configuration**: A target platform entry in `flui.toml` with scaffolded directory structure and build settings
- **Project Template**: A code generation blueprint that produces a compilable FLUI project from a name, organization, and dependency strategy

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All six CLI commands (`emulators`, `platform add`, `platform remove`, `devtools`, `create` templates, `run --hot-reload`) transition from stub/broken to functional
- **SC-002**: Developers can manage emulators for all supported mobile platforms through a single unified CLI interface
- **SC-003**: New projects generated via `flui create` compile successfully on first attempt
- **SC-004**: Integration test coverage for the flui-cli crate reaches at least 70%
- **SC-005**: All previously `NotImplemented` error returns are replaced with working implementations or clear, actionable "not available yet" messages with prerequisites

## Assumptions

- Android SDK and emulator CLI tools follow standard installation paths and output formats (`emulator -list-avds`, `adb devices`)
- Xcode's `xcrun simctl` output format is stable for parsing simulator information
- The `flui-devtools` crate will expose a public API for launching the DevTools server; the CLI will integrate once it's re-enabled
- Hot reload will use a file-system watcher approach (e.g., `notify` crate) rather than runtime VM-level hot patching
- Template dependency strategy defaults to crates.io for published releases, with `--local` as an opt-in development mode
- Platform scaffolding creates minimal directory structures; full platform-specific build files are handled by `flui-build`

## Scope Boundaries

**In scope:**
- Completing all stub commands to functional state
- Fixing template dependency resolution
- Adding integration test suite
- Making `flui-cli` ready to re-enable in the workspace

**Out of scope:**
- iOS build pipeline completion (separate concern in `flui-build`)
- Publishing FLUI crates to crates.io
- GUI-based DevTools (the CLI only launches the server)
- Cross-compilation toolchain installation (the `doctor` command already covers detection)
