// Print one line per on-screen window, for the macOS launch-render gate.
//
// `scripts/check-macos-launch-render.py` needs a window *number* to hand to
// `screencapture -l`, and the only non-interactive source for one is
// `CGWindowListCopyWindowInfo`. Neither `screencapture` nor any shell tool
// exposes that list, and the Python side cannot call it directly: the system
// Python ships without pyobjc (`import Quartz` fails), and re-binding the
// CoreFoundation object graph through `ctypes` is a lot of load-bearing FFI
// for one field.
//
// This is not a permission-gated read: window numbers, owners, layers and
// bounds are visible to any process. Window *titles* are redacted without
// Screen Recording, and deliberately not printed here.
//
// Owner name is matched by the caller; `layer=0` is a normal application
// window (`layer>0` is chrome — the Dock, the menu bar, notification centres),
// so a caller scanning for its own app should require layer 0.
//
// `origin` is printed alongside `size` because a window's *number* is only good
// for `screencapture -l`, which reads the window's own backing store — and a
// window that has been ordered front but has not yet drawn has none, so that
// capture returns a flat dark image whatever the display actually shows. Seeing
// what a person sees with an unpainted window therefore needs a screen capture
// of the window's rectangle, and that needs the origin. Both are in the global
// display coordinate space, which is also the space `screencapture -R` takes.

import CoreGraphics
import Foundation

let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []

for window in windows {
    let owner = window[kCGWindowOwnerName as String] as? String ?? ""
    let pid = window[kCGWindowOwnerPID as String] as? Int ?? -1
    let number = window[kCGWindowNumber as String] as? Int ?? -1
    let layer = window[kCGWindowLayer as String] as? Int ?? -1
    let bounds = window[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let x = bounds["X"] as? Double ?? 0
    let y = bounds["Y"] as? Double ?? 0
    let width = bounds["Width"] as? Double ?? 0
    let height = bounds["Height"] as? Double ?? 0
    print("pid=\(pid) win=\(number) layer=\(layer) size=\(Int(width))x\(Int(height)) "
        + "origin=\(Int(x)),\(Int(y)) owner=\(owner)")
}
