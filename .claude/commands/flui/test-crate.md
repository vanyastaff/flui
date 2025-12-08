---
name: Test Crate
description: Run tests for a specific FLUI crate with detailed output
---

Run tests for the specified FLUI crate: **$ARGUMENTS**

Steps:
1. Run `cargo test -p <crate_name> -- --nocapture` for full output
2. If tests fail, analyze the failure and suggest fixes
3. If no crate specified, run `cargo test --workspace`

Use `RUST_LOG=debug` for additional debugging output if needed.
