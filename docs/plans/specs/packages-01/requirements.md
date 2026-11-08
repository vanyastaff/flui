# Requirements: bounded facade application surface

- **Status:** Accepted
- **Date:** 2026-11-08
- **Finding:** `packages-01`

| ID | Requirement |
|---|---|
| R1 | `flui::app` must not re-export the whole `flui-app` crate, its bindings, embedder, runner or second prelude. |
| R2 | Ordinary launch and configuration types have their curated root/prelude paths; `flui::app` retains only the cross-thread application handle and host execution contracts needed there. Existing callers migrate in the same change. |
| R3 | Android's upstream `AndroidApp` and Android entry point are reachable only through Evolving `flui::native::android` behind `native`; no Stable root signature names `android-activity`. Generated applications enable `native` only for Android. |
| R4 | Facade API closure covers every supported target with all features. Existing unrelated AccessKit test-surface debt remains explicitly recorded rather than hidden. |
| R5 | Compile-fail and consumer tests pin removed and retained paths; formatting, Clippy, affected suites and repository gates pass. |
