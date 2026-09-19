---
paths: "**/Cargo.toml,.cargo/**,rust-toolchain.toml"
---

# Build & Toolchain Config

Rationale behind the build configuration. Read before changing a profile, a toolchain pin, or CI's build environment.

- **Toolchain:** development toolchain pinned in `rust-toolchain.toml` to `1.98.1` with `rustfmt` + `clippy` components. The pin is deliberately NOT the MSRV floor (`rust-version = "1.97"`), so a future MSRV freeze does not hold the developer back from stable diagnostics; only the `msrv` CI job exercises the floor.
- **Cargo profiles:** dev `opt-level = 1` (faster runtime) + `debug = "line-tables-only"` (backtrace file:line only — matches CI; variable/type DWARF was the bulk of `target/debug/deps`), deps `opt-level = 2` + `debug = false` (deps carry no debuginfo at all; raise it for one package to step into it — a global `-C debuginfo=` rustflag, from `RUSTFLAGS` or a user-level cargo config, overrides any `debug =` key silently and without error, since rustflags append after the profile flag); `dbg` profile (`inherits = "dev"`, `debug = "full"`) is the opt-in full-type-info build for a step-debugger; release `lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"` (the symbol table is retained so `perf`/flamegraph/minidumps can resolve frames — measured cost +935 KiB; DWARF from the std rlibs is still dropped, 23.5 MB → 4.19 MB).
- **Local disk:** `target/debug/deps` is the largest consumer on a 28-crate wgpu workspace (incremental is off via `[profile.dev] incremental = false` in the root `Cargo.toml` — `.cargo/config.toml`'s `[env] CARGO_INCREMENTAL = "0"` feeds sccache but does not itself reach cargo's profile resolution) — artifacts accumulate per RUSTFLAGS/feature/toolchain fingerprint with no size cap; run `just sweep` periodically (cargo-sweep: current-toolchain + 7-day prune).
- **CI:** sets `CARGO_INCREMENTAL=0` + `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and reclaims ~25 GB of runner bloat before building.
