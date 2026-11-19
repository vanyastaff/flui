### Removed

- `cargo xtask wasm-link`, `wasm-test`, `wasm-imports`, `wasm-test-crates` and `locked-version`:
  CI's `wasm-check` job builds the two web demos, runs `wasm-bindgen` over each and lists their
  import modules with node (only wasm-bindgen's own two may appear), and runs
  `cargo test --target wasm32-unknown-unknown` for each crate whose wasm32 dev-dependencies include
  `wasm-bindgen-test`, on node or in headless Chrome (ADR-0383 §5). The import allowlist file goes:
  the two allowed modules are named in the step.
