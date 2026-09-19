# macOS native text input (`NSTextInputClient`) — scout and design record

Status: **implemented 2026-09-17; review round applied; probe RUN AND PASSING on this Mac.**
`text_input.rs` and its wiring are in the tree and green on the crate (`--all-features` check,
clippy `-D warnings`, 62 macOS-module tests incl. 8 for the offset conversion and the `keyUp:`
gate). §2 is read from source; §4's decisions are what the code implements; §5.1's probe is the
step that turns any of it into a measurement — **and it has now run**: `just macos-ime` reaches
`IME_PROBE_RESULT=PASS` against real AppKit, with all five assertions measured and the fifth
mutation-checked (§8). What §6 withholds is unchanged: the probe synthesizes events, so a *genuine*
input method has still not driven a composition here.

## 1. Task and where it comes from

`docs/ROADMAP-TRACKER.md`'s App.5 — *"Full Material app on a native platform, real vsync
(`ControlFlow::Wait`), IME working"* — is `🛇 blocked`. `docs/ROADMAP.md` names the narrower
evidence gap in its honest-gaps list: *"real-IME evidence (ibus/fcitx CJK input on Linux, or
platform IME on Windows/macOS) is pending manual verification"*.

The capability itself is not missing from the framework: `PlatformInput::Ime(flui_types::ImeEvent)`,
the `PlatformTextInput` trait, and the widget-side composing-region rendering all exist
(ADR-0030, ADR-0032, ADR-0033). What is missing is **one backend's half of it** — and on macOS it is
missing entirely.

## 2. What is established (read from this tree, 2026-09-17)

**2.1 `PlatformTextInput` has four implementors and neither native backend is one of them.**

```
crates/flui-platform/src/platforms/winit/window.rs    WinitTextInput
crates/flui-platform/src/platforms/headless/platform.rs  FakeTextInput
crates/flui-interaction/src/text_input.rs             RecordingTextInput   (test double)
crates/flui-widgets/src/test_harness.rs               HarnessTextInput     (test double)
```

No `impl PlatformTextInput` exists under `platforms/macos/` or `platforms/windows/`.

**2.2 The macOS backend emits no `ImeEvent`, ever.** A repo-wide scan for
`NSTextInput`/`interpretKeyEvents`/`setMarkedText` returns **zero hits outside docs** — there is no
prior art, not even a stub. Consequently `PlatformInput::Ime` is never constructed on macOS and
`PlatformWindow::text_input` has nothing to return.

**2.3 Plain Latin typing does work, and the mechanism matters for the fix.** `extract_key`
(`platforms/macos/events.rs:224-257`) reads `[NSEvent characters]` and returns
`Key::Character(chars)`, falling back to a physical-key table first
(`shared::keys_macos::keycode_to_key`). So ASCII and the U.S./Russian layouts produce characters
through the *keyboard* path.

**2.4 The content view is a bare `NSView` subclass.** `get_or_create_view_class`
(`platforms/macos/view.rs:164-350`) registers `FLUIContentView` with `acceptsFirstResponder`,
`become`/`resignFirstResponder`, `keyDown:`, `keyUp:`, `flagsChanged:`, the mouse and scroll
handlers, `isOpaque`, `acceptsTouchEvents`, `dealloc`. No protocol conformance, no
`interpretKeyEvents:`. `keyDown:` and `keyUp:` are both wired to `handle_input_event`
(`:259-266`), which converts and dispatches directly.

**2.5 The consequence, stated exactly.** On the native macOS backend:

| | today |
|---|---|
| Latin characters into a text field | works, via `Key::Character` |
| Dead keys (e.g. `´` + `e`) | no — the input method never engages |
| CJK / any composing input method | no |
| `Preedit` / `Commit` / `Enabled` / `Disabled` | never emitted |
| Candidate-window placement | no — nothing can call `firstRectForCharacterRange:` |
| `set_ime_allowed` / `set_ime_cursor_area` | not implementable; the trait is unimplemented |

So `docs/ROADMAP.md`'s gap is understated for this backend: this is not "IME pending manual
verification", it is "no IME exists to verify". On macOS the only path that has IME today is the
**winit fallback** (`winit-backend` feature, optional on macOS — `platforms/mod.rs:28-32`), whose
`WinitTextInput` is a two-line delegate to `winit::Window::set_ime_allowed` /
`set_ime_cursor_area`.

## 3. The reference shape

**In tree.** `WinitTextInput` (`platforms/winit/window.rs`) is deliberately thin because winit owns
the platform work. On macOS there is nothing to delegate to: AppKit's input method is a *protocol
the view must implement*, so the thin-wrapper shape does not transfer — the backend becomes the
implementor.

**Platform contract.** AppKit's composition pipeline is entered only by
`-[NSView interpretKeyEvents:]`, which forwards the event to the view's input context and calls
*back* into the view's `NSTextInputClient` methods: `setMarkedText:selectedRange:replacementRange:`
while composing, `insertText:replacementRange:` on commit, `doCommandBySelector:` for keys the
input method declines. The candidate window's position is *queried* from the view through
`firstRectForCharacterRange:actualRange:` — which is exactly what
`PlatformTextInput::set_ime_cursor_area` must eventually answer, and what ADR-0032's
single-rect reduction already fixes the FLUI-side shape of.

## 4. The decisions this slice owes (each one is a hazard, not a detail)

**D1 — `keyDown:` must not gain a second producer.** This is the slice's central risk. Today every
`keyDown:` becomes one `KeyboardEvent`. If `interpretKeyEvents:` is called unconditionally, the
*same* key press also produces `ImeEvent::Commit` from `insertText:` — one physical key, two events
into the app, which is the identical defect ADR-0044 §3 records for `record_compositor_tick` (a
per-pump call that must not mark demand). The rule the design needs: the input-context route is
taken **only while a text input is attached** (`set_ime_allowed(true)`), and in that state a
character key is delivered as composition/commit and *not* also as `Key::Character`.
`doCommandBySelector:` is the escape hatch that keeps arrows, Escape and friends on the keyboard
path. Nothing in this tree decides that today; it has to be decided here.

**D2 — the view must own marked-text state, because AppKit queries it.**
`hasMarkedText`, `markedRange`, `selectedRange`, `attributedSubstringForProposedRange:actualRange:`
and `validAttributesForMarkedText` are read *by* the input method, not pushed to it. `ViewContext`
(`view.rs:75-78`) currently holds `{ scale_factor, callbacks }` only, so the marked text, its
selection, and the last `set_ime_cursor_area` rect all need a home there. The existing `context_ptr`
ivar is the natural one — but see D4.

**D3 — `set_ime_cursor_area` has no effect without `firstRectForCharacterRange:`.** They are one
mechanism: the setter stores, the getter answers. Implementing the trait method without the
conformance would be a stub that changes nothing — which is why this slice cannot be split into a
"capability half" first.

**D4 — owner-lane discipline.** Every `NSTextInputClient` callback arrives on the main thread by
AppKit's own guarantee, so the view's own state needs no lane hop. `PlatformTextInput` is a public
`Send + Sync` capability, though, so `set_ime_cursor_area` can be called from any thread — it must
go through the same `route_on_owner` throat every other AppKit window-site uses
(`platforms/macos/owner_lane.rs`; the sweep that established this is issue #1194).

**D5 — the deployment floor: checked against the real SDK, and it is not an obstacle.**
Read from `MacOSX.sdk`'s `AppKit.framework/Headers/NSTextInputClient.h` and `NSTextInputContext.h`
(2026-09-17): **not one method this design needs carries an availability annotation** —
`insertText:replacementRange:`, `setMarkedText:selectedRange:replacementRange:`, `unmarkText`,
`selectedRange`, `markedRange`, `hasMarkedText`, `attributedSubstringForProposedRange:actualRange:`,
`validAttributesForMarkedText`, `firstRectForCharacterRange:actualRange:`,
`characterIndexForPoint:`, `doCommandBySelector:`, plus `NSTextInputContext`'s `handleEvent:`,
`invalidateCharacterCoordinates`, `discardMarkedText` and `currentInputContext`. The only
annotated members are ones this design does not use (`drawsVerticallyForCharacterAtIndex:`, 10.6;
`insertAdaptiveImageGlyph:`, 15.0). So nothing here is gated on raising the 11.0 floor — which is
the opposite of the display-link situation the pacing plan's §6 item 3 records, where the modern
spelling *is* above the floor.

## 5. How this slice is verified (Mac-only, and executable here)

The route is already established in this repo for AppKit work: a bundled `.app` probe, because an
unbundled `NSWindow` throws `_CFBundleGetValueForInfoKey` and a bare `cargo test` cannot host AppKit
windows. `just macos-frame-pump`, `just macos-close-path`, and `just macos-resize-jitter` are the
three working precedents.

Two arms, and the second is what makes the first meaningful:

1. **Protocol arm (always runnable).** Drive the view's `NSTextInputClient` methods directly, as an
   input method would — `setMarkedText:` then `insertText:` — and assert the emitted
   `ImeEvent::Preedit`/`Commit` sequence and the answers to `hasMarkedText`/`markedRange`.
2. **End-to-end arm (needs a real input method).** Send the view a real `keyDown:` through
   `interpretKeyEvents:` and assert what AppKit actually calls back. This machine's enabled input
   sources (`defaults read com.apple.HIToolbox AppleEnabledInputSources`, 2026-09-17) are U.S. and
   Russian keyboard layouts plus `com.apple.PressAndHold` and `com.apple.CharacterPaletteIM` —
   `PressAndHold` is a genuine composing input method (hold a letter, pick an accented variant), so
   a real composition is reachable here without installing a CJK source. If it proves undrivable
   from a probe, that arm is recorded as **not driven**, not quietly dropped.

The `U.S.` layout arm alone already crosses the boundary that matters: with it, a plain letter
`keyDown:` still goes through `interpretKeyEvents:` → `insertText:` → our client, so the *wiring* is
exercised even when no composition occurs.

### 5.1 The probe, concretely (design settled 2026-09-17; written to match the harness that exists)

Files, mirroring `just macos-frame-pump`'s route exactly:

| file | role |
|---|---|
| `crates/flui-platform/examples/ime_probe.rs` | the probe; `#[cfg(target_os = "macos")] mod appkit_ime_probe { pub(crate) fn run() }` + the two-cfg `fn main()` |
| `crates/flui-platform/examples/Info.plist.ime_probe` | the bundle plist (clears `_CFBundleGetValueForInfoKey`) |
| `justfile` recipe `macos-ime` | build → stage into `target/macos-ime/ImeProbe.app` → run → assert exit 0 + `IME_PROBE_RESULT=PASS` |

Harness shape, taken from `frame_pump_probe.rs` (read, not guessed): `MacOSPlatform::new()` →
`Box::new(platform).run(Box::new(|owner| { … }))` → `owner.open_window(WindowOptions{ visible: true, … })`
→ `pending.try_ready()`. A **visible** window is required (an unactivated app gets no input routing).
`tracing_subscriber` init at the top; `fatal()` exits 1 and prints `IME_PROBE_RESULT=FAIL`.

Everything the probe needs is **public API plus AppKit** — no crate internals, no new test seam:

- `window.on_input(cb)` collects every `PlatformInput` into a shared `Vec` (the `ImeEvent` observer).
- `window.text_input()` yields the `Arc<dyn PlatformTextInput>` (must be `Some` on macOS after this
  slice; a `None` here is itself a failure worth reporting).
- The view is reached **through AppKit**, not through the crate: `[[NSApp windows] firstObject]`
  → `contentView`. That is how the probe plays input method.

**The assertions, and which one is the point.**

1. **Mutual exclusion — this is the ADR-0069 assertion, and the reason the probe exists.** With
   `set_ime_allowed(true)`, synthesize a real `NSEvent` for an ordinary letter via
   `+[NSEvent keyEventWithType:…characters:@"a"…]` and send it to the view's `keyDown:`. Assert
   **exactly one** `PlatformInput::Ime(Commit("a"))` and **zero** `Key::Character`. Then
   `set_ime_allowed(false)`, repeat, and assert the **exact inverse**: one `Key::Character`, zero
   `ImeEvent`. A probe that only checked the first half would pass on an implementation that
   double-produces.
2. **Protocol conformance.** Drive `setMarkedText:selectedRange:replacementRange:` then
   `insertText:replacementRange:` directly, and assert the `Preedit` → `Commit` sequence, plus the
   answers to `hasMarkedText` / `markedRange` / `selectedRange` (read back via `msg_send!`).
   Include one non-ASCII case so the UTF-16 → byte-offset helper is exercised on the wire, not only
   in its unit tests.
3. **Cursor area.** `set_ime_cursor_area(...)` then read `firstRectForCharacterRange:actualRange:`
   and assert the rect comes back non-zero and screen-relative.
4. **The composition that ends without a commit.** `setMarkedText:` (non-empty) then `unmarkText`,
   asserting **exactly one** `ImeEvent::Preedit` with an empty `text` and `cursor: None`, and
   `hasMarkedText` false afterwards. Added after the implementation landed, because the
   implementation chose an empty `Preedit` over emitting nothing and that choice carries the only
   design decision in the change with no other executable test behind it: a client left holding
   composition state it was never told ended suppresses `Key::Character` for the rest of the focus
   session. The implementation agent flagged this gap itself.
5. **Drop, not commit, on disable.** `set_ime_allowed(false)` clears the marked state as well as
   emitting `Disabled`, so a composition in flight at that moment produces no `Commit`. Asserted as
   an absence — the contract ADR-0069 records — so the probe would fail if the backend began
   committing.
6. **The release, and its gate** (added after the first four assertions passed, because the probe
   as first written sent only `keyDown:`). `keyUp:` is a separate AppKit entry point gated on the
   *composition*, not on attachment: with a text input attached and nothing composing, a
   synthesized `keyUp:` must arrive as exactly one release (`KeyState::Up`); with a composition
   open the same call must produce nothing. The reporting arm runs first so the suppressing arm's
   absence is measured on a channel just proven live; the event is judged on its `KeyState`, since
   an implementation that ran the `keyDown:` path from `keyUp:` would pass an arrival-only check.

7. **A modified key's route** (added later, like arm 6). A press carrying *both* a modifier and a
   non-ASCII character — Option-`E` — must arrive as an input-method event with the keyboard path
   silent, and must not leave a composition open. Arms 1–6 press either a plain letter or nothing at
   all, so they cannot see an implementation that keyed its routing on the modifier or on the
   character being ASCII. This arm was written hoping the layout would *compose* the press (see §10,
   which measured that it does not here); it now asserts the route and reports the characters.

**Honest limits to write into the probe's own module doc, not to discover later:** arm 2's
synthesized `NSEvent` is not a human keystroke and does not run a genuine input method — it proves
the *routing*, which is what ADR-0069 decides. A real composition (press-and-hold, or a CJK source)
stays **not driven** until a real one is used, and the probe must say so rather than imply the IME
gap is closed. §6's "what is not claimed" still stands against this probe, and §10 is the attempt
that was made against it plus the measurement that leaves it standing.

## 6. What is not claimed here

- Nothing in §2 was measured by execution; it is read from source. The probe in §5 is what would
  turn any of it into a measurement.
- The Windows backend has the same missing capability (§2.1). It is out of reach on this machine
  and is not part of this record.
- `docs/ROADMAP.md`'s IME gap is *not* closed by this record. It stays open until a real input
  method drives a real composition end to end. §10 records the attempt to close it with a layout's
  own dead-key table, the four probes that measured why that does not work here, and the exact
  precondition that would have to change.

## 7. Review round (2026-09-17) — what changed after the first implementation

An adversarial review of the diff returned four findings. All four are closed, and two of them
turned out to have a different true shape than the review stated. Recorded here because the
reasoning is the interesting part, not the patch.

**7.1 The `get_ivar` guard was asymmetric (blocker, and it was real).**
`update_view_scale_factor` read the `context_ptr` ivar off a view the caller had fetched with
`-[NSWindow contentView]`, while the new `with_view_context` helper beside it checked the class
first and its doc named the unguarded read as forbidden. `liquid_glass` leaves an
`NSVisualEffectView` as the content view, and `objc` 0.2.7's `get_ivar` **panics** on a missing
ivar — fatally, across an AppKit `extern "C"` frame. Fixed by extracting the class check into
`content_view_context_ptr` and giving it a `&mut` sibling (`with_view_context_mut`), so both
readers share one guarded entry point rather than one guarded and one not.

**7.2 The `keyUp:` finding was *half* right, and the half that was right had a better fix.**
The review read the ungated `keyUp:` as contradicting winit's documented agreement, and offered
two options: gate it, or narrow the sentence. Reading winit 0.30.13's actual macOS `view.rs`
(`key_up`, the `Ground | Disabled` test) showed the docs describe only the *press*: winit gates
the release too, but on `ime_state`, not on `ime_allowed` — and because a commit resets that state
to `Ground` inside `keyDown:`, a plain Latin character's release **is** delivered. So the review's
premise (that winit emits no release while IME is attached) is false, and a gate on `ime_allowed`
would have been wrong; but the narrower true defect — a release landing *inside an open
composition* — is real, and winit's own condition fixes it. `key_up` now gates on
`TextInputState::reports_key_release`, and the predicate is a pure function with a
mutation-checked test (the `!ime_allowed`-only variant fails it on the attached-but-idle case).
The ARCHITECTURE.md lineage paragraph was rewritten to cite the implementation, not just the docs.

**7.3 `firstRectForCharacterRange:actualRange:` answered a not-found range beside a real rect.**
The SDK header (`NSTextInputClient.h`, read on this machine) is explicit: *"If non-NULL,
actualRange contains the character range corresponding to the returned area."* The callback wrote
`{NSNotFound, 0}` and then returned a real screen rect. Now the failure paths keep `{NSNotFound,
0}` (there is no area for a range to correspond to) and the success path writes the range that goes
with the area it just built — the composition range while composing, a zero-length range at the
view's own origin otherwise, the same origin `markedRange` reports from.

**7.4 A SAFETY comment listed `B@:` for `hasMarkedText`.** Correct on arm64, wrong in spelling for
x86_64 (`c@:`) — `BOOL`'s encoding is target-dependent and `objc`'s own `Encode for BOOL` is what
supplies it. The comment now says so instead of asserting one spelling.

**7.5 A nested `keyDown:` could clobber `pending_key_event`.** An input method that routes a second
`keyDown:` through the view while the outer one is inside `interpretKeyEvents:` (the character
palette does) left the outer event's `doCommandBySelector:` reading zero, dropping a command
belonging to the outer press. Now saved and restored rather than set and cleared.

**Still open from §5:** the probe had not run when this round was written. It has since run and
passed — see §8, which supersedes this line.

## 8. The probe's first run (2026-09-17) — what it measured, and what it does not

`just macos-ime` reports `IME_PROBE_RESULT=PASS`. The class-registration block — the
`add_protocol` call and all 11 method registrations, hand-checked against `objc` 0.2.7's
`add_method` preconditions until now — has executed against real AppKit without throwing, which is
the first fact in this document that was not read from source.

Measured values, as logged by the probe itself:

| assertion | measurement |
|---|---|
| A (attached) | `commits=["a"] typed=[]` — one press, one semantic event |
| A (disable) | composition dropped (`Preedit{"a",(0,1)}` collected, not committed), `on_disable=[Disabled]` |
| A (detached) | `typed=["a"] ime=[]` — the exact inverse |
| B (composition) | `preedits=[("に", Some((0, 3)))]`, `marked_range=(0, 1)` — "に" is **1 UTF-16 unit and 3 bytes**, so the conversion is exercised on the wire |
| B (commit) | `commits=["に"]`, `has_marked_text=false` |
| C | `firstRectForCharacterRange:` → `1490.0,1103.0 2×18`, `actualRange` answered `{0, 0}` |
| D | `after_unmark=[Preedit { text: "", cursor: None }]`, `has_marked_text=false` |
| E (reported) | `released=["a"]` at `state: Up` |
| E (suppressed) | `while_composing=[]` — the bounded wait ran its full 500 ms |

**The fifth assertion was written after the first four passed**, because the probe as first run
sent only `keyDown:` and so exercised neither of §7's two behaviour fixes. It now covers
`actualRange:` (assertion C passes a real out-param and asserts the answer) and the `keyUp:` gate
(assertion E). E is ordered reporting-then-suppressing on purpose: an absence only means something
on a channel just proven live on the same window, and the reporting arm uses the identical call.

**E is mutation-checked.** Replacing `key_up`'s `reports_key_release()` with an unconditional
`true` makes the suppressing arm fail with exactly one leaked
`KeyboardEvent { state: Up, key: Character("a"), code: KeyA, … }` while the reporting arm still
passes — so the assertion measures the gate and nothing else. The gate was restored and the probe
re-run to `PASS`.

**What this does not close.** The events are synthesized with `+[NSEvent keyEventWithType:…]` and
delivered by messaging the view directly; no genuine input method runs. §6's list stands unamended:
the ROADMAP's IME gap needs a real composition (press-and-hold or a CJK source) end to end, and
that has not been driven. **§10 supersedes the last sentence's optimism**: the dead-key route was
tried, on both enabled layouts, and measured to produce no composition — with the layout tables
read directly to show the premise was not simply wrong about the U.S. layout. It remains undriven,
now with a named precondition rather than a plan to try again.

**One API wart found, not fixed here.** `KeyboardEvent::state` is a public field whose type
(`keyboard_types::KeyState`) is not re-exported by `flui_platform::traits`, so a consumer can read
the field but cannot name its type. The probe names the dependency directly rather than reaching
through a re-export that does not exist. Widening the re-export set is a public-API decision that
belongs to its own change, not to this slice.

## 9. The Flutter cross-check (2026-09-17) — three citations corrected, and what each one was

Verifying this slice's divergence claims against a freshly restored reference at the pinned tag
(`.flutter` at `3.44.0`; the clone had been sitting on the default branch, which is exactly the
failure `AGENTS.md` warns about) found **three written claims that the reference does not support**,
in the two directions of error. All three are corrected in place, in shipped files:

1. **"Flutter's `TextInputConnection.connectionClosed` commits"** — written in
   `crates/flui-platform/ARCHITECTURE.md` and again in **ADR-0069's Consequences bullet**. True in
   effect, false as stated, and the first correction of it was false the other way.
   `EditableTextState.connectionClosed` (`editable_text.dart:4138`) nulls the connection, drops
   `_lastKnownRemoteTextEditingValue` and unfocuses — no commit call anywhere. The unfocus routes
   through `_openOrCloseInputConnectionIfNeeded` to `controller.clearComposing()`, and
   `clearComposing` (`editable_text.dart:378`) assigns only `composing: TextRange.empty` — the
   controller's `text` is **untouched**. So the composed characters survive as ordinary text; what
   Flutter does *not* do is announce a commit, which is the input method's own last
   `updateEditingValue` before the close. My intermediate rewrite ("leaves the composing region
   exactly as it was") was wrong on the region and right on the announcement; both shipped sites now
   carry the verified mechanism with line numbers.
2. **"winit's documentation settles the attachment rule but says nothing about disabling
   mid-composition"** — true of the doc, and it understated the support for the decision. winit's
   macOS *implementation* does exactly what FLUI does: `set_ime_allowed`
   (`winit-0.30.13/src/platform_impl/macos/view.rs:880`) clears `marked_text` before setting
   `ImeState::Disabled` and queueing `WindowEvent::Ime(Ime::Disabled)`. The public doc for the call
   (`src/window.rs:1265`) states only "the window won't receive `Ime` events, and will receive
   `KeyboardInput` for every keypress". ADR-0069 now cites the implementation, and says why it is
   the implementation rather than the doc.
3. **`PlatformTextInput::set_ime_allowed`'s own doc** (`traits/text_input.rs`) said the drop
   "follows winit's own semantics" and described the Flutter side only as "keeps the uncommitted
   text". That claim checks out against winit's source and is now stated with the mechanism, so the
   trait doc, the ADR and `ARCHITECTURE.md` no longer give three different accounts of one
   behaviour.

**Why this is recorded here rather than only in the diff.** Each of the three was a *grounding*
claim — the thing that makes a deliberate divergence a decision rather than an accident — and two of
them were written before the reference was available at its pinned tag. The divergences themselves
were never in question: FLUI drops the composition on disable, Flutter keeps the characters, and the
client-side bug class in `flui-types/src/ime.rs` is what makes the drop the right call. What changed
is that the decision now rests on a verified reading of both references instead of a recollection.

## 10. The dead-key route to a real composition (2026-09-17) — measured, and it does not open one here

§6 and §8 both end on the same open item: no genuine input method drives a composition, so the
probe's own `setMarkedText:`/`insertText:` calls are still standing in for the thing under test.
This section is the attempt to close it, and the measurement that says it stays open.

**The attempt.** Assertion F was written to let AppKit compose for itself: press Option-`E` on the
built-in U.S. layout, where it is a dead key, then `e`, and require exactly one commit of `é` — a
character *neither press carried*. Two presses of `e` cannot yield `é` unless something composed
them, and nothing in the probe's hands would have. It was the strongest claim available without a
CJK source.

**It is false on this host, in two independent ways.** The measurement had to be taken off the Rust
build (`just ci` held cargo's lock), so it was taken with four standalone `clang` probes — kept
under `probes/`, with their own README carrying the build and run commands. In order:

| probe | question | answer |
|---|---|---|
| `sources.m` | which keyboard input sources exist? | **Two, both raw keylayouts** (`com.apple.keylayout.US`, `com.apple.keylayout.Russian`); no source of type `kTISTypeInputMethod` |
| `deadkey_live.m` | does the sequence compose? | **No** — `insertText:` `´`, then `insertText:` `e`, zero `setMarkedText:`, `hasMarkedText=0` |
| `deadkey_us_selected.m` | …because Russian was active? | **No** — identical with the U.S. layout explicitly selected (`TISSelectInputSource`, status 0, restored after) |
| `layout_tables.m` | …because the U.S. layout has no dead key? | **No** — its own tables give Option-`E` as `""` with `deadKeyState=1`, and the following `e` resolving to `é` |

So the layout defines the composition, the active input source is one that would use it, and the
input *context* still does not engage it. The fourth row matters most: it rules out the reading that
the premise was simply wrong about the U.S. layout, which is the reading I would have preferred.
The conversion engine an input method provides is what is absent, and the first row is what rules
out retrying with a different source — there is no other source to select. Enabling one is a System
Settings change, not something a probe may make on its user's behalf.

**What changed in the shipped probe.** F now asserts the *route* and reports the characters. Both
presses must arrive as input-method events with the keyboard path silent, and no composition may be
left open afterwards; the characters themselves are logged, not required. That is a real extension
of coverage rather than a retreat — assertion A presses one unmodified letter, and F is the only
assertion in the probe that presses a key carrying *both* a modifier and a non-ASCII character, the
one shape an implementation that keyed its routing on either property would get wrong. What F no
longer does is claim a composition ran, because requiring the layout's resolved `é` would pin a
property of the host as a contract of this backend and fail for a reason that is not a defect. The
`COMPOSED_E_ACUTE` constant is gone with the claim.

**What is still open, stated so it cannot be mistaken for closed.** The ROADMAP's IME gap needs a
real input method driving a real composition end to end, and that has not happened. The precondition
is now named precisely rather than gestured at: **the host must have an input source of type
`kTISTypeInputMethod` enabled**, which this machine does not. Press-and-hold accent selection and
the candidate window are in the same state, and a CJK source would exercise the byte/UTF-16
conversion through a path no assertion here reaches.

**One thing this did not do.** Unlike assertion E, the reshaped F is not mutation-checked — no
variant of the backend was built to confirm the new assertion fails when the route it asserts is
broken. E earned that treatment because its gate has a plausible one-line wrong variant; F's route
does not have one as obvious. It is recorded as a gap rather than papered over with a plausible-
sounding claim of having checked.
