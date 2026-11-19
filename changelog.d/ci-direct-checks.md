### Removed

- `cargo xtask docs-links` and `cargo xtask checks --strict`: CI's `checks` job runs
  `cargo fmt --all --check`, `typos`, `taplo fmt --check` and lychee as steps of its own, and
  `cargo xtask checks` keeps the toolchain pin, unmounted tests, ADR numbers, changelog fragments
  and generated files. The tasks that wrapped one standard tool are gone too: `lint`, `gate`,
  `test`, `platform-test`, `cli-test`, `wayland-optional`, `build-all-targets`, `ci`, `ci-full`,
  `deps`, `native-features`, `gpu-test`, `miri`, `doc-strict`, `feature-matrix`, `facade-combos`
  and `wasm-check`; `docs/testing.md` names the commands each CI job runs (ADR-0383).
