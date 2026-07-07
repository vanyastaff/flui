# Panic Policy

> When FLUI code is allowed to panic, what a panic message must say, and how
> the `clippy::unwrap_used` gate enforces the boundary. Applies to every crate
> in the workspace. Companion to the error-handling split in `AGENTS.md`
> (`thiserror` in libraries, `anyhow` in applications).

## The rule

A panic is a **bug report**, never a control-flow mechanism.

| Situation | Required handling |
|---|---|
| Caller-triggerable failure (bad input, missing file, lock contention, platform error) | Return `Result` with a `thiserror` error type. Never panic. |
| Internal invariant that the module itself maintains (slab index handed out by this arena, ID minted by this tree, state machine transition guarded upstream) | `expect("BUG: <invariant that was violated>")` |
| Test code, benches, examples | `unwrap()`/`expect()` freely — a panic *is* the failure report there. |
| Compile-time-checkable invariant | Prefer the type system (`NonZeroUsize` IDs, Arity system) over any runtime check. This is the house style — see the ID offset pattern in `AGENTS.md`. |

### The `BUG:` message convention

Every production-path `expect()` documents the invariant it asserts, prefixed
with `BUG:` so a user hitting it knows immediately the fault is FLUI's, not
theirs, and grep can find every invariant assertion in the workspace:

```rust
// YES — states the invariant, identifies the owner of the bug
let node = self.nodes
    .get(index)
    .expect("BUG: RenderId minted by this tree must resolve in its slab");

// NO — restates the operation, blames nobody
let node = self.nodes.get(index).expect("failed to get node");

// NO — caller-triggerable, must be a Result
let file = std::fs::read(path).expect("BUG: asset must exist");
```

`unwrap()` carries no message and is therefore **never** acceptable on a
production path — if the invariant is real, name it with `expect("BUG: …")`;
if you cannot name it, it is not an invariant and the path needs a `Result`.

Panics inside `unsafe` contexts deserve extra scrutiny: an `expect()` whose
failure would leave a raw-pointer structure half-updated must either be
hoisted above the unsafe region or the SAFETY comment must cover the
unwind path.

## Enforcement

`[workspace.lints.clippy]` sets `unwrap_used = "warn"`, and the clippy CI job
runs with `-D warnings`, so an unannotated production `unwrap()` fails CI.
The root `clippy.toml` sets `allow-unwrap-in-tests = true` and
`allow-expect-in-tests = true`, so `#[test]` functions and `#[cfg(test)]`
modules are exempt automatically.

**Transitional state (ship-quality waves).** Crates that predate this policy
carry a tracked crate-level opt-out:

```rust
#![allow(clippy::unwrap_used)] // TODO(ship-wave-N): burn down per docs/PANIC-POLICY.md
```

Each quality wave removes the allow for its cohort by converting every
`unwrap()` to either a `Result` path or a `BUG:`-message `expect()`. New
crates must not add the opt-out; new code in existing crates should conform
even while the crate-level allow is still present.

`expect()` is deliberately **not** linted (`expect_used` stays off): with the
`BUG:` convention it is the sanctioned invariant idiom, and the review bar is
the message, not the call. Audit them with:

```bash
rg '\.expect\("(?!BUG: )' crates/*/src --pcre2   # expects missing the convention
rg '\.unwrap\(\)' crates/*/src                   # should be test-only or allow-tracked
```
