# Tasks: example, template and acceptance parity

- **Status:** Complete
- **Date:** 2026-11-08
- **Requirements:** [requirements.md](requirements.md)
- **Design:** [design.md](design.md)

| Task | State | Validation evidence |
|---|---|---|
| T1. Unify counter template, example and agent acceptance | done | Shared source included by all three consumers. Acceptance r871: 3 passed; cli_create r832: 11 passed; generated counter compile r861 passed; example tests r856: 2 passed; strict Clippy r865/r874 passed. Deliberate `+= 2` mutation failed acceptance r869 before restoration |
| T2. Remove the hand-written hot-reload counter and compile the generated workspace | done | Deleted all eight project files; generated hot-reload workspace targeted compile test passed; facade-only template dependency test passed; hot-reload suite: 9 passed, 6 ignored; reach and reach self-test passed; xtask: 66 passed |
| T3. Move internal fixtures beside owning crates and add package examples | done | Internal examples moved beside their owners; SDK-only Material and Cupertino examples ran headlessly and compiled for wasm32. Owning-crate suites, lifecycle fixture, strict Clippy, workspace and reach passed. Combined Material/Cupertino r897: 80 passed, 8 ignored |
| T4. Synchronize documents and changelog | done | Commands and paths name the owning-crate locations; remaining old-root names are confined to archival specs. `packages-09-example-parity.md` records the replacement |
| T5. Run final affected suites and repository gates | done | `cargo fmt --all -- --check` passed; full `xtask checks` r899 passed, including 0 documentation-link errors, 29 crates/235 edges and 73 reach roots; `git diff --check` passed |
