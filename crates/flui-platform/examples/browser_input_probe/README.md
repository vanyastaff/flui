# Browser platform contracts

Build `cargo build --locked -p flui-platform --example browser_input_probe --target wasm32-unknown-unknown`.
Run the wasm-bindgen CLI version matching Cargo.lock over
`target/wasm32-unknown-unknown/debug/examples/browser_input_probe.wasm`, with
`--target web --out-dir target/browser-input-probe`. Copy the sibling `index.html`
into that output directory and serve it over localhost HTTP.

Use browser device emulation with a device pixel ratio of 2. Install input, then
check pointer/wheel events and display bounds. Each check reports PASS or FAIL
from the translated public results. These are actual DOM events dispatched to
the platform canvas; physical mouse hardware delivery is a separate smoke test.

Run each callback check in a fresh page with an external deadline. A backend
that calls a handler while holding its mutex can block or abort the wasm
instance before the page can report a result. The fixture exercises reentrant
clipboard access; default wasm panic-abort builds do not establish Rust panic
containment.

The probe keeps no browser preference changes: reset device emulation and close
the page after collecting its results. This manual browser contract is not
claimed as an automatically executed CI lane.

The wheel-unit button sends DOM Pixels, Lines and Pages deltas with fractional
double precision and checks the public event's exact units, values and modifiers.
The keyboard button checks actual DOM delivery of named keys, Cyrillic text,
physical codes, locations, repeat/composition and normalized releases. These
constructor-driven probes establish translation behavior, not physical keyboard
layout or wheel hardware fidelity.

The cancellation and capture-loss buttons check one terminal cancellation and
the next healthy sequence. The fractional-position button checks genuine DOM
pointer and wheel events at fractional CSS coordinates. The trusted capture
button arms a physical/browser-automation drag: press the canvas, move beyond
its rectangle and release. Set a bounded canvas CSS size first when the page
uses the default full-viewport canvas. `capture-admission` must report PASS and
the outside release must reach `capture-check`; synthetic DOM pointer events
cannot prove capture admission because they do not create an active pointer.

WheelEvent constructors can coerce fractional viewport coordinates to integers.
The fractional wheel case therefore uses integer source coordinates and a
fractional canvas origin with explicit borders and padding. Its transformed
case checks that native local coordinates remain authoritative; browser rounding
on transformed wheel coordinates is not repaired by guessing an inverse from
an axis-aligned bounding rectangle.

The pen sample check supplies coalesced and predicted lists of actual DOM
PointerEvent objects through explicitly overridden methods; the DOM constructor
has no arguments for those lists. Each source object's target, fractional
coordinates, readings and timestamp are checked before the platform receives
the parent. This exercises the public producer's ordering and sensor conversion,
and does not establish physical pen hardware delivery. The sensor validation
check separately supplies invalid or absent readings and optional methods.

The trusted mouse sample check reads the browser's own source methods during
real pointer movement, without source overrides. It compares the public samples
with those native lists and prints the actual source values. An empty history
or prediction list proves only the empty-list path on that browser; the fixture
does not claim hardware predictions were observed. The native chord check pins
the pointermove button edges between the first press and final release.

The getter reentry check dispatches a second DOM pointerdown synchronously from
the first event's buttons getter. Run it on a fresh page: a recursive mutable
JavaScript closure or an active-state borrow held across that getter can abort
the wasm instance. Both presses and their releases must reach the public input
callback, with no browser window error.

Keyboard and wheel getter reentry each have a fresh-page check. A native key
or delta getter synchronously dispatches an inner event of the same family;
the public callback must receive the inner event before the outer one, then
deliver a healthy follow-up. These listeners capture immutable handles, so
their wasm closures do not acquire a recursive `FnMut` guard.
