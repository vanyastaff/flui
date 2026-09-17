QUESTION: Does FLUI's winit backend preserve physical keyboard identity when converting to canonical W3C `KeyboardEvent.code` values?

ANSWER: No. The winit backend maps only a narrow `KeyCode` subset to `keyboard_types::Code` and falls back to `Code::Unidentified` for many physical keys that both winit and the canonical W3C vocabulary can identify, including numpad, punctuation/Intl, media/browser, and higher function-key families. Filed #1092.

VERSIONS:
- flui local workspace at current audit state, 2026-09-13.
- winit 0.30.13 from `Cargo.lock`.
- keyboard-types 0.8.3 from `Cargo.lock`.
- ui-events 0.3.0 from `Cargo.lock`.

SOURCES:
- `crates/flui-platform/src/platforms/winit/events.rs:417-498`: `convert_physical_key` maps letters, top-row digits, a small navigation/modifier subset, and `F1..F12`, then falls through to `Code::Unidentified`.
- `crates/flui-platform/src/platforms/winit/events.rs:500-533`: `convert_location` already recognizes several `KeyCode::Numpad*` variants for `Location::Numpad`, so the same event can carry `location = Numpad` while `code = Unidentified`.
- `crates/flui-platform/src/platforms/winit/events.rs:557-580`: `keyboard_event` publishes the lossy code in `KeyboardEvent`.
- `crates/flui-platform/src/shared/keys.rs:293-389`: Win32 mapping already covers numpad, Intl, media/browser, context-menu, and higher function-key families.
- `crates/flui-platform/src/shared/keys_macos.rs:257-299`: AppKit mapping already covers numpad, media, Intl, context-menu, and higher function-key families.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-0.30.13/src/keyboard.rs:296-620`: winit exposes `KeyCode` variants for punctuation/Intl, numpad, media/browser/system, and extended key families.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/keyboard-types-0.8.3/src/code.rs:220-360` and nearby lines: `keyboard_types::Code` exposes matching W3C variants such as `Numpad*`, `Browser*`, `AudioVolume*`, and `F13+`.

ISSUE:
- https://github.com/vanyastaff/flui/issues/1092

TESTS:
- `cargo nextest run -p flui-platform --features winit-backend --lib -E 'test(platforms::winit::events::) or test(shared::scroll::) or test(shared::keys::tests::) or test(shared::keys_macos::tests::)' --no-fail-fast`
- Result: 39 passed, 140 skipped. This confirms existing pointer/scroll/IME/native key tests pass, but the winit keyboard conversion family lacks coverage for the failing physical-key classes.

OPEN:
- Need implementation design: extend winit mapping locally or centralize a shared W3C key mapping policy with backend-specific exceptions.
- Need acceptance tests for representative winit keys: numpad, Intl/punctuation, context menu, media/browser, and F13+.

ANSWERED
