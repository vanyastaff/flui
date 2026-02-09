# Implementation Plan: flui-cli Completion

**Branch**: `001-cli-completion` | **Date**: 2026-02-08 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-cli-completion/spec.md`

## Summary

Complete all stub/placeholder commands in `flui-cli`, fix template dependency resolution, and add integration tests. The CLI already has a well-architected foundation with type-safe newtypes, builder-pattern command runners, and comprehensive error handling. This plan fills the gaps: implementing the emulators command (Android AVD + iOS simulator management via external CLI tools), platform add/remove (directory scaffolding + `flui.toml` mutation), fixing templates to use correct dependency declarations, connecting the devtools stub to `flui-devtools` when available, adding file-watching hot reload, and adding integration tests via `assert_cmd`.

## Technical Context

**Language/Version**: Rust 1.91 (workspace `rust-version`)
**Primary Dependencies**: clap 4.5, cliclack 0.3.6, console 0.15, flui-build, flui-log, toml 0.9, which 8.0, serde, thiserror, tracing, dirs 5.0
**New Dependencies Needed**: `notify 7.x` (file watcher for hot reload)
**Storage**: File-based (`flui.toml` project config, `~/.flui/config.toml` global config)
**Testing**: `assert_cmd` 2.0 + `predicates` 3.1 + `tempfile` 3.10 (already in dev-deps)
**Target Platform**: Cross-platform CLI (Windows, macOS, Linux)
**Project Type**: Single crate (binary) within a Cargo workspace
**Performance Goals**: CLI commands should complete in < 2s (excluding external tool invocations like emulator launch)
**Constraints**: No `unwrap()` in library code, `tracing` for all logging, `thiserror` for errors
**Scale/Scope**: ~15 source files modified/added, ~800 new lines of implementation + ~400 lines of tests

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Flutter as Reference, Not Copy | PASS | CLI is FLUI-specific tooling, not a Flutter port |
| II. Strict Crate Dependency DAG | PASS | `flui-cli` depends on `flui-build` and `flui-log` only; no new intra-crate deps added |
| III. Zero Unsafe in Widget/App Layer | PASS | CLI is in Tools layer; no `unsafe` needed |
| IV. Composition Over Inheritance | PASS | Existing patterns use builder pattern + traits |
| V. Declarative API, Imperative Internals | N/A | CLI is a tool, not widget API |
| Rust Standards: No unwrap/println | PASS | All code uses `CliResult<T>`, `thiserror`, `tracing` |
| Rust Standards: Strict clippy | PASS | Workspace lints inherited |
| Git Workflow | PASS | Feature branch off main, conventional commits |
| Testing: Coverage thresholds | TARGET | CLI falls under "Platform" category = 70% minimum |
| ID Offset Pattern | N/A | No slab-based IDs in CLI |
| Node Storage: No Arc<Mutex<>> | N/A | No tree structures in CLI |

**Gate result**: PASS - no violations.

## Project Structure

### Documentation (this feature)

```text
specs/001-cli-completion/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output (CLI command contracts)
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
crates/flui-cli/
├── Cargo.toml                    # Add: notify dependency (hot reload)
├── src/
│   ├── main.rs                   # Modify: update Emulators subcommand to have list/launch subcommands
│   ├── commands/
│   │   ├── emulators.rs          # REWRITE: implement Android AVD + iOS simulator management
│   │   ├── devtools.rs           # REWRITE: implement devtools server launch (or graceful unavailable message)
│   │   ├── platform.rs           # MODIFY: implement add() and remove() functions
│   │   └── run.rs                # MODIFY: add file watcher for --hot-reload
│   ├── templates/
│   │   ├── basic.rs              # MODIFY: fix dependency declarations
│   │   └── counter.rs            # MODIFY: fix dependency declarations
│   └── utils.rs                  # MODIFY: add platform scaffolding helpers
├── tests/
│   ├── cli_create.rs             # NEW: integration tests for create command
│   ├── cli_doctor.rs             # NEW: integration tests for doctor command
│   ├── cli_completions.rs        # NEW: integration tests for completions command
│   ├── cli_platform.rs           # NEW: integration tests for platform command
│   └── cli_errors.rs             # NEW: integration tests for error cases
```

**Structure Decision**: Single crate, existing layout preserved. New integration tests go in `tests/` directory following Cargo convention. No new modules needed beyond test files.

## Constitution Re-Check (Post Phase 1 Design)

| Principle | Status | Notes |
|-----------|--------|-------|
| II. Strict Crate Dependency DAG | PASS | Optional `flui-devtools` dep flows downward (CLI → DevTools). `notify` is external. No cycles. |
| III. Zero Unsafe in Widget/App Layer | PASS | No `unsafe` in any planned code. All external tool interaction via `std::process::Command`. |
| Rust Standards: Minimal dependencies | PASS | `notify-debouncer-mini` is small and justified for hot reload. `flui-devtools` is optional. |
| Testing: 70% coverage target | PLAN | 5 integration test files + existing unit tests should exceed 70% |
| Anti-patterns: No println/dbg | PASS | All output through `cliclack` and `tracing` |

**Post-design gate result**: PASS - no new violations from design decisions.

## Complexity Tracking

No constitution violations to justify.
