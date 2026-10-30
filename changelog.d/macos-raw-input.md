### Fixed

- macOS: a modifier key pressed or released alone is a key event of its own, read from `flagsChanged:` side by side (left and right Shift, Control, Option, Command; Caps Lock per toggle; Fn).
- macOS: Control+letter reports the letter as its logical key instead of a C0 control character, so Control shortcuts match.
- macOS: a key released while Command is held reaches the window, so Command+K no longer leaves K held.
- macOS: a mouse release lost while the window was not key is taken back (`PointerEvent::Cancel`) on the next plain move, and the next press of that button is a down again. The shared pointer assembly does this for every backend that reports the held set on a move.
- macOS: fingers coming to rest on the trackpad (`MayBegin`) are `PointerEvent::ScrollInertiaCancel` instead of a phaseless zero-delta scroll, and the cancel that follows them is no orphan `ScrollPhase::Cancelled`; the shared scroll grammar gives no phase to a cancel of a gesture that never began.
- macOS: `mouseCancelled:` takes a press back.
