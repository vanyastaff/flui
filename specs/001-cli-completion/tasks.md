# Tasks: flui-cli Completion

**Input**: Design documents from `/specs/001-cli-completion/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cli-commands.md, quickstart.md

**Tests**: Integration tests are included as User Story 4 (P2) per the feature specification (FR-013..FR-015).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

All paths relative to `crates/flui-cli/`:
- Source: `src/`, `src/commands/`, `src/templates/`
- Tests: `tests/`
- Config: `Cargo.toml`

---

## Phase 1: Setup

**Purpose**: Re-enable flui-cli in the workspace and prepare dependencies

- [x] T001 Uncomment `"crates/flui-cli"` in workspace `Cargo.toml` members list to re-enable the crate
- [x] T002 Add `notify` and `notify-debouncer-mini` dependencies to `crates/flui-cli/Cargo.toml` for hot reload support
- [x] T003 Add optional `flui-devtools` dependency behind `devtools` feature flag in `crates/flui-cli/Cargo.toml`
- [x] T004 Verify `cargo check -p flui-cli` compiles cleanly after dependency changes

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared scaffolding logic that both `flui platform add` (US1) and `flui create` (US3) depend on

**CRITICAL**: Platform scaffolding function must be complete before US1 and US3 can proceed

- [x] T005 Create platform scaffolding module with `scaffold_platform(platform: &str, project_dir: &Path) -> CliResult<()>` function in `crates/flui-cli/src/utils.rs` — generates platform-specific directories and config files under `platforms/<name>/` (Android: `AndroidManifest.xml`; iOS: `Info.plist`; Web: `index.html`; Desktop: empty marker directories)
- [x] T006 Add `valid_platform_names() -> &[&str]` helper function in `crates/flui-cli/src/utils.rs` returning `["android", "ios", "web", "windows", "linux", "macos"]` for validation by both `platform add` and `create --platforms`

**Checkpoint**: Scaffolding utilities ready — user story implementation can now begin

---

## Phase 3: User Story 1 — Emulator and Simulator Management (Priority: P1)

**Goal**: Developers can list and launch Android emulators and iOS simulators from the CLI

**Independent Test**: Run `flui emulators list` on a machine with Android SDK installed — should display AVD names with status. Run `flui emulators launch <name>` — should start the emulator.

### Implementation for User Story 1

- [x] T007 [US1] Refactor `Emulators` variant in `Commands` enum in `crates/flui-cli/src/main.rs` — replace `launch: Option<String>` with a `PlatformSubcommand`-style enum having `List` and `Launch { name: String }` subcommands
- [x] T008 [P] [US1] Implement Android AVD listing in `crates/flui-cli/src/commands/emulators.rs` — run `emulator -list-avds` (parse line-delimited output), cross-reference with `adb devices -l` for running status, return `Vec<Emulator>` structs
- [x] T009 [P] [US1] Implement iOS simulator listing in `crates/flui-cli/src/commands/emulators.rs` — run `xcrun simctl list devices --json` (parse JSON), filter by `isAvailable: true`, return `Vec<Emulator>` structs. Guard with `cfg!(target_os = "macos")` and runtime `which xcrun` check
- [x] T010 [US1] Implement unified `execute_list(platform_filter: Option<String>)` in `crates/flui-cli/src/commands/emulators.rs` — merge Android and iOS results, display as a formatted table using `cliclack` with columns: Platform, Name, Version, Status
- [x] T011 [US1] Implement `execute_launch(name: &str)` in `crates/flui-cli/src/commands/emulators.rs` — search by name across Android AVDs and iOS simulators, launch via `emulator -avd <name>` (background process) or `xcrun simctl boot <udid>`, report success/failure with spinner
- [x] T012 [US1] Add error handling for missing SDKs in `crates/flui-cli/src/commands/emulators.rs` — when `emulator`/`adb` not found, display installation guidance for Android SDK; when `xcrun` not found or not on macOS, display appropriate message
- [x] T013 [US1] Update `main.rs` command dispatch in `crates/flui-cli/src/main.rs` — route `Commands::Emulators` subcommands to `emulators::execute_list()` and `emulators::execute_launch()`

**Checkpoint**: `flui emulators list` and `flui emulators launch <name>` fully functional

---

## Phase 4: User Story 2 — Platform Scaffolding (Priority: P1)

**Goal**: Developers can add and remove platform support via `flui platform add/remove`

**Independent Test**: In a FLUI project, run `flui platform add android` — should create `platforms/android/` with `AndroidManifest.xml` and update `flui.toml`. Run `flui platform remove android` — should remove the directory after confirmation and update `flui.toml`.

### Implementation for User Story 2

- [x] T014 [US2] Rewrite `add()` function in `crates/flui-cli/src/commands/platform.rs` — validate platform names using `valid_platform_names()`, load `FluiConfig` via `FluiConfig::load()`, check for duplicates, call `scaffold_platform()` from utils, update `build.target_platforms` in config, save via `FluiConfig::save()`, display progress with `cliclack` spinners
- [x] T015 [US2] Rewrite `remove()` function in `crates/flui-cli/src/commands/platform.rs` — load `FluiConfig`, verify platform exists in `target_platforms`, prompt for confirmation via `cliclack::confirm()`, delete `platforms/<name>/` directory, update and save config, handle edge case of last remaining platform with extra warning
- [x] T016 [US2] Add unit tests for platform add/remove in `crates/flui-cli/src/commands/platform.rs` — test: valid platform validation, duplicate detection, invalid platform name error, config serialization after add/remove

**Checkpoint**: `flui platform add/remove` fully functional, `flui platform list` unchanged

---

## Phase 5: User Story 3 — Template Dependency Resolution (Priority: P2)

**Goal**: Generated projects use correct dependency declarations and scaffold `platforms/` directories

**Independent Test**: Run `flui create test-app` — the generated `Cargo.toml` should use crates.io version specifiers. Run `flui create test-app --local` — should use path dependencies. Generated `platforms/` directories should match `flui.toml` target_platforms.

### Implementation for User Story 3

- [x] T017 [US3] Add `--local` flag to `Commands::Create` variant in `crates/flui-cli/src/main.rs` — boolean flag, default false, passed through to template generation
- [x] T018 [P] [US3] Rewrite `generate_cargo_toml()` in `crates/flui-cli/src/templates/basic.rs` — accept `local: bool` parameter; when false, emit `flui_app = "0.1"` etc. (crates.io); when true, emit `path = "../../crates/..."` (local paths); add `# FLUI Template v{VERSION}` comment using `env!("CARGO_PKG_VERSION")`
- [x] T019 [P] [US3] Rewrite `generate_cargo_toml()` in `crates/flui-cli/src/templates/counter.rs` — same logic as T018 for counter template
- [x] T020 [US3] Update `generate()` functions in both `crates/flui-cli/src/templates/basic.rs` and `crates/flui-cli/src/templates/counter.rs` — accept `local: bool` and `platforms: &[String]` parameters, call `scaffold_platform()` for each platform in the list after generating the project structure
- [x] T021 [US3] Update `TemplateBuilder` in `crates/flui-cli/src/templates/mod.rs` — add `.local(bool)` and `.platforms(Vec<String>)` builder methods, pass them through to template `generate()` calls
- [x] T022 [US3] Update `commands::create::execute()` in `crates/flui-cli/src/commands/create.rs` — pass `local` flag and `platforms` (from `--platforms` arg or default from generated `flui.toml`) to `TemplateBuilder`
- [x] T023 [US3] Update `commands::create_interactive::interactive_create()` in `crates/flui-cli/src/commands/create_interactive.rs` — add platform selection step in the interactive wizard, return platforms in the config struct

**Checkpoint**: `flui create` generates compilable projects with correct dependencies and platform directories

---

## Phase 6: User Story 4 — Integration Test Suite (Priority: P2)

**Goal**: Comprehensive integration tests verify CLI commands work end-to-end

**Independent Test**: Run `cargo test -p flui-cli` — all integration tests pass, covering create, doctor, completions, platform list, and error cases.

### Implementation for User Story 4

- [x] T024 [P] [US4] Create `crates/flui-cli/tests/cli_create.rs` — test `flui create test-project` in `TempDir` (verify directory structure: `Cargo.toml`, `src/main.rs`, `flui.toml`, `assets/` exist), test `flui create test-project --template basic` (verify basic template content), test `flui create test-project --local` (verify path dependencies in `Cargo.toml`)
- [x] T025 [P] [US4] Create `crates/flui-cli/tests/cli_errors.rs` — test `flui create fn` (Rust keyword, expect non-zero exit), test `flui create 123bad` (leading digit, expect error), test `flui create` with no name and no interactive (expect error or interactive prompt)
- [x] T026 [P] [US4] Create `crates/flui-cli/tests/cli_doctor.rs` — test `flui doctor` (expect exit code 0, output contains "Rust" or "cargo"), test `flui doctor --verbose` (expect more detailed output)
- [x] T027 [P] [US4] Create `crates/flui-cli/tests/cli_completions.rs` — test `flui completions bash` (expect valid bash completion script on stdout), test `flui completions powershell` (expect valid PowerShell script)
- [x] T028 [P] [US4] Create `crates/flui-cli/tests/cli_platform.rs` — test `flui platform list` (expect exit code 0, output contains "Android", "iOS", "Web")

**Checkpoint**: `cargo test -p flui-cli` passes all integration tests; coverage target >= 70%

---

## Phase 7: User Story 5 — DevTools Launcher (Priority: P3)

**Goal**: `flui devtools` launches the DevTools server when available, or shows clear instructions

**Independent Test**: Run `flui devtools` without the devtools feature — should display instructions. With the feature enabled, should attempt to start a server.

### Implementation for User Story 5

- [x] T029 [US5] Rewrite `execute()` in `crates/flui-cli/src/commands/devtools.rs` — use `#[cfg(feature = "devtools")]` to conditionally compile: when enabled, call `flui_devtools` server startup on the specified port; when disabled, display formatted message with instructions on how to enable the feature (`cargo install flui-cli --features devtools`)
- [x] T030 [US5] Add port-in-use detection in `crates/flui-cli/src/commands/devtools.rs` — attempt `TcpListener::bind` on the specified port before starting the server, display clear error if port is occupied with suggestion to use `--port <other>`

**Checkpoint**: `flui devtools` provides clear behavior in both enabled and disabled states

---

## Phase 8: User Story 6 — Hot Reload (Priority: P3)

**Goal**: `flui run --hot-reload` watches for file changes and automatically rebuilds/restarts the application

**Independent Test**: Start a FLUI app with `flui run --hot-reload`, modify `src/main.rs`, verify the app rebuilds and restarts automatically.

### Implementation for User Story 6

- [x] T031 [US6] Implement `watch_and_rebuild()` function in `crates/flui-cli/src/commands/run.rs` — set up `notify_debouncer_mini::new_debouncer` with 500ms debounce watching `src/` recursively and `Cargo.toml`, on change: kill current child process → run `cargo build` → spawn new `cargo run` child process
- [x] T032 [US6] Integrate hot reload into `execute()` in `crates/flui-cli/src/commands/run.rs` — when `hot_reload` is true: perform initial build, spawn app as `std::process::Child`, enter watch loop; when false: delegate to `CargoCommand::run_app()` as before
- [x] T033 [US6] Add graceful shutdown handling in `crates/flui-cli/src/commands/run.rs` — handle `Ctrl+C` (via `ctrlc` crate or signal handling) to kill child process and exit watch loop cleanly; handle build failures by displaying error and continuing to watch without crashing

**Checkpoint**: `flui run --hot-reload` works end-to-end with file watching, rebuild, and restart

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Final cleanup, linting, and validation

- [x] T034 Run `cargo clippy -p flui-cli -- -D warnings` and fix any warnings across all modified files
- [x] T035 Run `cargo fmt --all` to ensure formatting compliance
- [x] T036 Run `cargo test -p flui-cli` to verify all unit and integration tests pass
- [x] T037 Validate quickstart.md checklist — manually verify each item in `specs/001-cli-completion/quickstart.md` verification checklist

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies — start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 — scaffolding utilities used by US1 removal and US3 create
- **Phase 3 (US1 Emulators)**: Depends on Phase 2 — but independent of other user stories
- **Phase 4 (US2 Platform)**: Depends on Phase 2 — uses `scaffold_platform()` from foundational
- **Phase 5 (US3 Templates)**: Depends on Phase 2 and Phase 4 — reuses platform scaffolding logic and `FluiConfig` patterns from US2
- **Phase 6 (US4 Tests)**: Depends on Phases 3, 4, 5 — tests exercise all implemented commands
- **Phase 7 (US5 DevTools)**: Depends on Phase 1 only — independent of other user stories
- **Phase 8 (US6 Hot Reload)**: Depends on Phase 1 only — independent of other user stories
- **Phase 9 (Polish)**: Depends on all desired stories being complete

### User Story Dependencies

- **US1 (Emulators)**: Independent — can start after Phase 2
- **US2 (Platform add/remove)**: Independent — can start after Phase 2
- **US3 (Templates)**: Depends on US2 patterns — `scaffold_platform()` and `FluiConfig` save/load
- **US4 (Integration Tests)**: Depends on US1, US2, US3 — tests need working commands
- **US5 (DevTools)**: Independent — can start after Phase 1
- **US6 (Hot Reload)**: Independent — can start after Phase 1

### Within Each User Story

- Foundation utilities before command implementations
- Core functionality before error handling
- Implementation before tests
- Story complete before moving to next priority

### Parallel Opportunities

**After Phase 2 completes, these can run in parallel**:
- US1 (Emulators) and US2 (Platform) — different files, no shared state
- US5 (DevTools) and US6 (Hot Reload) — different files, no shared state
- Within US1: T008 (Android listing) and T009 (iOS listing) — different code paths
- Within US4: All test files (T024-T028) — independent test files
- Within US3: T018 (basic template) and T019 (counter template) — different files

---

## Parallel Example: User Story 1

```bash
# After Phase 2, launch Android and iOS listing in parallel:
Task: "T008 [P] [US1] Implement Android AVD listing in crates/flui-cli/src/commands/emulators.rs"
Task: "T009 [P] [US1] Implement iOS simulator listing in crates/flui-cli/src/commands/emulators.rs"

# After both complete, unified display and launch:
Task: "T010 [US1] Implement unified execute_list()"
Task: "T011 [US1] Implement execute_launch()"
```

## Parallel Example: User Story 4

```bash
# All integration test files can be created in parallel:
Task: "T024 [P] [US4] Create crates/flui-cli/tests/cli_create.rs"
Task: "T025 [P] [US4] Create crates/flui-cli/tests/cli_errors.rs"
Task: "T026 [P] [US4] Create crates/flui-cli/tests/cli_doctor.rs"
Task: "T027 [P] [US4] Create crates/flui-cli/tests/cli_completions.rs"
Task: "T028 [P] [US4] Create crates/flui-cli/tests/cli_platform.rs"
```

---

## Implementation Strategy

### MVP First (US1 + US2 Only)

1. Complete Phase 1: Setup (T001-T004)
2. Complete Phase 2: Foundational (T005-T006)
3. Complete Phase 3: US1 Emulators (T007-T013)
4. Complete Phase 4: US2 Platform add/remove (T014-T016)
5. **STOP and VALIDATE**: Both commands work independently
6. This delivers the two P1 stories — the most impactful CLI improvements

### Incremental Delivery

1. Setup + Foundational → Dependencies ready
2. US1 (Emulators) → Test independently → Commit
3. US2 (Platform) → Test independently → Commit
4. US3 (Templates) → Test independently → Commit
5. US4 (Tests) → Validate all prior work → Commit
6. US5 (DevTools) → Test independently → Commit
7. US6 (Hot Reload) → Test independently → Commit
8. Polish → Final validation → PR

### Parallel Team Strategy

With two developers after Phase 2:
- Developer A: US1 (Emulators) → US3 (Templates) → US6 (Hot Reload)
- Developer B: US2 (Platform) → US5 (DevTools) → US4 (Tests)

---

## Notes

- All paths are relative to `crates/flui-cli/` unless otherwise specified
- T008 and T009 touch the same file (`emulators.rs`) but different functions — can be parallelized as long as file structure is coordinated
- US5 (DevTools) depends on `flui-devtools` crate being re-enabled for full functionality; without it, only the "not available" message path is testable
- US6 (Hot Reload) introduces `notify` as a new external dependency — verify it doesn't conflict with existing deps
- Constitution compliance: no `unwrap()`, no `println!`, all logging via `tracing`, errors via `thiserror`
