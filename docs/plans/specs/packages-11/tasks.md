# Tasks: shared capability execution and conformance

- **Status:** In implementation
- **Date:** 2026-11-08
- **Requirements:** [requirements.md](requirements.md)
- **Design:** [design.md](design.md)

| Task | State | Validation evidence |
|---|---|---|
| T1. Wire the existing host executor through platform, runtime, app and headless services | done | `r948` platform contract: 1 passed; `r951` initial/secondary runtime wiring: 2 passed; `r952` deterministic headless lane: 1 passed; `r953` app executor retirement: 1 passed; strict four-crate Clippy `r956` passed |
| T2. Unify scripted and portal behavior into typed conformance scenarios | done | One crate-level table drives eight launcher/dialog scenarios through public acquisition and HeadlessHost for scripted and Linux portal adapters. Windows flui-system suite: 7 passed; strict test Clippy passed. Linux adapter remains unrun because the C toolchain is unavailable |
| T3. Replace per-runtime portal workers with shared bus/executor and local cancellation | in progress | Implementation pending |
| T4. Add multi-runtime regressions and write architecture/current-rule records | pending | Implementation pending |
| T5. Run affected suites, strict Clippy and repository gates | pending | Validation pending |
