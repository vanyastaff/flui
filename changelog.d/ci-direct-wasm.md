### Removed

- `cargo xtask wasm-link`, `wasm-test`, `wasm-imports`, `wasm-test-crates` and `locked-version`:
  CI's `wasm-check` job builds the two web demos and runs `cargo test --target wasm32-unknown-unknown`
  for each crate whose wasm32 dev-dependencies include `wasm-bindgen-test`, on node or in headless
  Chrome (ADR-0383 §5). The import allowlist goes with `wasm-imports`: since Rust 1.96 an undefined
  symbol fails the wasm link instead of becoming an import.
