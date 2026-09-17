QUESTION: Does Flutter's invalid-Unicode text-input crash issue reveal an analogous FLUI text-input defect?

ANSWER: Not as a confirmed FLUI defect. Flutter issue #179727 is a useful policy lesson for text bridges, but FLUI's core text input vocabulary stores `String`, which cannot contain unpaired UTF-16 surrogates, and the inspected Windows text paths reject malformed UTF-16 before creating `Key::Character` text. No issue filed.

EXTERNAL SOURCE:
- Flutter issue #179727, open as of 2026-09-13: invalid Unicode in `TextEditingController.text` can crash on iOS/macOS through JSON/platform-channel decoding. Comments discuss sanitizer-vs-fail-fast policy and note that malformed strings should not crash production, but framework behavior may remain undefined after sanitization.

LOCAL SOURCES:
- `crates/flui-types/src/ime.rs:72-87`: `ImeEvent::Preedit` and `ImeEvent::Commit` carry Rust `String`, so the framework-level event vocabulary cannot represent unpaired UTF-16 surrogate halves.
- `crates/flui-platform/src/shared/keys.rs:426-445`: Windows `WM_CHAR` burst decoding uses `String::from_utf16(units).ok()?` and returns `None` for invalid UTF-16 rather than manufacturing text.
- `crates/flui-platform/src/shared/keys.rs:499-525`: out-of-band `WM_CHAR` handling assembles surrogate pairs and drops lone/mismatched halves.
- `crates/flui-platform/src/platforms/windows/platform.rs:1317-1337`: stray `WM_CHAR` dispatch only sends text when `assemble_stray_wm_char` returns `Some(text)`.
- `crates/flui-platform/src/platforms/macos/events.rs:240-255`: current AppKit keyboard extraction copies `NSString.UTF8String` through `CStr::to_str`; invalid UTF-8 is treated as no key text, not as malformed text payload.
- `crates/flui-platform/src/platforms/web/events.rs:336-376`: web keyboard events come through JS string APIs and `keyboard-types` parsing; `is_composing` is preserved.

LESSON:
- FLUI should keep platform text input boundaries explicit about malformed external text: either reject it before reaching framework state, or replace it with a documented signal. Silent lossy conversion at IME/text-editing boundaries would recreate Flutter's ambiguous production behavior.
- This is especially important for future iOS/macOS/Android IME implementations, not just keyboard events.

TESTS:
- No new executable test was run for this note. It is a source-policy disposition, not a reproduction.

OPEN:
- Future native IME implementations should add tests for malformed UTF-16/UTF-8 at the platform boundary where such malformed input is representable.

ANSWERED
