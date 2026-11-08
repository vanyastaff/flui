### Changed

- The dlopen worker-host development cycle and Android scene plugins are supported development workflows under ADR-0316, not temporary scaffolding for a code patcher. Platform loader dependencies and per-image owner-thread state now have explicit standing permissions; process-global worker registries remain tracked for per-loop ownership.

### Removed

- The unused `flui_hot_reload::ReloadStrategy` descriptor, also formerly reachable as `flui::hot_reload::ReloadStrategy`. Reload orchestration and the shared `strategy::{env, timing}` constants are unchanged.
