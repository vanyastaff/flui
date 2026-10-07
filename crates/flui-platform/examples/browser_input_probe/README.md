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
