### Removed

- `cargo xtask wasm-link`, `wasm-test`, `wasm-imports`, `wasm-test-crates` and `locked-version`:
  CI's `wasm-check` job builds the two web demos, checks their imports with `wasm-tools print`
  against `.github/wasm-import-allowlist.txt`, and runs `cargo test --target wasm32-unknown-unknown`
  on node for each crate with a wasm32 `wasm-bindgen-test` dev-dependency (ADR-0383 §5).
  `cargo xtask doctor full` names the wasm-tools version the job installs.
