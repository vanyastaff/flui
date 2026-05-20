# Claude Code hooks for FLUI — requirements

**Created:** 2026-05-19
**Status:** v1 scaffold
**Scope:** Lightweight — wire defensive PreToolUse / PostToolUse hooks borrowed from `oven-sh/bun#30412` rewrite pattern.

## Problem

FLUI is mid-port from Flutter (Dart) → Rust. The `oven-sh/bun#30412` rewrite of Bun from Zig+C++ → Rust (merged 2026-05-14) demonstrated that AI coding agents benefit from project-level guardrails:

- agents run slow commands (`cargo test --workspace` workspace-wide) when a scoped invocation would do
- agents inconsistently format files between edits, polluting diffs
- agents bypass rules creatively — wrapping forbidden commands in `timeout`, redirect, or pipe (the bun team famously commented `// Claude is a sneaky fucker` next to the wrap-strip layer)

FLUI today has no `.claude/settings.json` and no `.claude/hooks/`. Every cargo invocation runs unbounded; `.rs` files are not auto-formatted; nothing catches sneak-bypass.

## Goal

Ship a minimal defensive hook pair so that:

1. **Slow / dangerous cargo invocations are denied** with a fix-it message before the Bash tool runs them.
2. **`.rs` files are auto-formatted** via `cargo fmt -- <file>` after every Write / Edit / MultiEdit.
3. **Bypasses fail loudly** — the deny layer strips common wrapping patterns (`timeout`, `> file`, `2>&1`, `|`, inline env like `RUST_LOG=debug …`) before checking the inner command.

## Non-goals (v1)

- Constitution-level guards (no `unsafe` outside `flui-{platform,painting,engine}`, no `unwrap()`/`println!`/`dbg!` in widget/app layer). Need path-aware regex over file content and a whitelist — defer to v2.
- Slash commands (`/flui-port-widget`, `/flui-sweep`, `/flui-parity`) — separate skill work.
- CLAUDE.md upgrade with `.flutter` source-of-truth rule — covered by a separate doc pass.
- Workspace-wide `cargo fmt` orchestration; we only format the file the agent just touched.

## Acceptance criteria

- `.claude/settings.json` exists at repo root with `PreToolUse:Bash` and `PostToolUse:Write|Edit|MultiEdit` hook entries pointing at PowerShell scripts under `.claude/hooks/`.
- `.claude/hooks/pre-bash-cargo.ps1` denies the following invocations:
  - `cargo test --workspace` without an explicit `-p <crate>` filter
  - `cargo build --release` (allow `--profile release` only when called from CI scripts via documented env)
  - `cargo test` or `cargo build` wrapped in `timeout … cargo …`
  - The same wrapped in `… | grep …`, `… > out.log`, `… 2>&1`
  - The same prefixed with inline env (`RUST_LOG=debug …`, `CARGO_TARGET_DIR=… …`)
- `.claude/hooks/post-edit-fmt.ps1` runs `cargo fmt -- <abs path>` on the changed file if it ends in `.rs` and exits 0 silently on success, prints stderr on failure but never blocks.
- Both scripts are tested via fake JSON stdin: deny path emits the documented JSON shape with `permissionDecision: "deny"` + reason; fmt path runs `cargo fmt` only on `.rs` writes.

## Runtime choice

PowerShell. FLUI's CLAUDE.md names PowerShell as the primary shell on the user's Windows 11 environment, and PS 7+ runs on macOS / Linux for cross-platform contributors. Bun's hooks use the Bun runtime (dogfooding); the FLUI equivalent would be a `flui-hooks` Rust binary, but that imposes a compile latency on every Bash tool call. PS scripts execute in tens of milliseconds and need no project-level build step.

## Open questions / v2 backlog

- Track every deny-bypass we observe and append the strip rule. The bun list grew organically; we should be ready to do the same.
- Consider a `FLUI_HOOK_OFF=1` env opt-out per session for debugging cases where the hook itself is wrong.
- Once v1 lands, evaluate Approach C (unsafe / panic / log guards) once we have a few weeks of agent-touched edits to learn from.

## References

- `oven-sh/bun#30412` — "Rewrite Bun in Rust", merged 2026-05-14
- `.claude/settings.json` and `.claude/hooks/*.js` in `oven-sh/bun@main` — pattern source
- `STRATEGY.md` § "Our approach" — port-not-redesign principle
- `CLAUDE.md` § "Logging" — `tracing` only rule that v2 may enforce
