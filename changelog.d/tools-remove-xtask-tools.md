### Removed

- `cargo xtask worktree`, `doctor`, `bench-collect`, `clean-nested`, `toolchain`, `unmounted-tests`
  and `adr-numbers`, and the Native xtask workflow. The MSRV, unmounted-test-file and ADR-number
  checks are steps of CI's `checks` job; `git worktree`, criterion's `--save-baseline` with
  `critcmp`, and the job that runs a tool take the place of the rest.

### Changed

- `test-desktop`, which nextest starts each Windows test through, is the crate
  `tools/test-desktop` (`cargo build -p test-desktop`).
