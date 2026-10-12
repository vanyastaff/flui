# Web canvas input ownership and registration lifetime

Status: Approved, 2026-10-12; implemented, release checks pending. Baseline is merged
`ba1044c664c56b4df5682b09ac6634a6ffe1d01f`. This feature addresses #1049 and
#1050 together; unrelated input follow-ups remain outside this scope.

## Problem Statement

An embedded FLUI canvas receives keys typed into unrelated HTML inputs. Its
focus callback and public focus getters disagree. Keyboard callback outcomes
do not stop DOM propagation or prevent browser defaults. Closing a window leaves
callbacks, listeners and their resources attached to the page, so reopening
delivers the same key to both retired and current registrations.

Current production browser evidence in Chrome 154.0.8037.99, with live public
window/platform owners and fixture-only changes:

- A focused host input receives trusted `b` as text; FLUI also receives both
  key states. Synthetic host Down likewise reaches FLUI.
- An initially unfocused canvas reports focused/active=true. After real canvas
  focus and host-input focus, callback=false but both getters remain true.
- Six of eight Down/Up dispatch-bit combinations fail; the two permissive
  controls pass. Prevented trusted Tab still moves focus into the host input,
  and the global listener incorrectly delivers that input's Up to FLUI.
- Close/free keeps the callback-only Arc witness alive. A subsequent key goes
  to the retired registration; reopen then delivers one key to both owners.

These are producer defects. Web startup's explicit Resumed state, focus reentry,
partial registration failure and close followed by unwind are additional
source leads/acceptance obligations, not newly reproduced baseline failures.

## Solution

Give one live canvas registration explicit ownership of its DOM subscriptions.
Only its focused canvas admits keyboard activity. Publish coherent focus facts,
apply existing dispatch outcomes to DOM, and retire all sources and captures
when that registration closes. Keep the retained logical widget focus target
so returning to the canvas restores normal shortcuts and text behavior.

## User Stories

1. As an embedding author, I want host input typing to bypass FLUI, so that the same key cannot edit HTML and activate a framework action.
2. As an embedding author, I want textarea and contenteditable input isolated, so that ordinary page controls retain their browser behavior.
3. As an application user, I want focused canvas keys delivered once, so that shortcuts have one observable effect.
4. As an application author, I want initial focus to reflect the DOM, so that constructing a canvas does not invent focus or steal it.
5. As a focus observer, I want getters updated before notification, so that one callback sees a coherent observation.
6. As an application user, I want native Tab to leave the canvas when allowed, so that embedded HTML remains keyboard accessible.
7. As an application author, I want consumed Tab to keep browser focus when prevented, so that framework traversal and browser traversal do not both run.
8. As a host script author, I want propagation and cancellation independent, so that consuming an event need not suppress its default action.
9. As a host script author, I want same-target observers retained, so that framework consumption does not commandeer every listener on the element.
10. As an application user, I want focus return followed by a healthy shortcut, so that focus loss does not strand state or replay activation.
11. As a package author, I want logical primary focus retained across native focus loss, so that a restored window resumes the same focused control.
12. As an embedding author, I want an owned canvas moved into open or closed shadow DOM to retain routing, so that identity survives DOM retargeting.
13. As an embedding author, I want unrelated shadow inputs excluded, so that a host or ancestor match does not grant ownership.
14. As an application author, I want reentrant getter and callback delivery preserved, so that callbacks can use the same public handle safely.
15. As an application author, I want close from an input/focus/resize callback safe, so that teardown cannot invalidate an executing closure.
16. As an embedding author, I want close to release captures and observer registrations, so that retired renderers and callbacks do not accumulate.
17. As an embedding author, I want close/reopen to install one fresh owner, so that historical registrations cannot duplicate events.
18. As an application author, I want repeated close to be idempotent, so that notifications and resource retirement occur once.
19. As an embedding author, I want my canvas retained after FLUI closes, so that framework teardown does not delete my DOM.
20. As an application author, I want failed listener installation rolled back, so that retry cannot inherit a half-installed window.
21. As an application author, I want unsupported ResizeObserver to use the existing resize fallback explicitly, so that missing capability is distinguishable from silent partial registration.
22. As an application author, I want concurrent ownership of the same canvas refused, so that opening a second registration cannot silently displace the first.
23. As a contributor, I want browser fixtures to hold explicit owners, so that tests do not depend on leaked closures.
24. As a contributor, I want Unicode and existing pointer contracts preserved, so that lifetime repair does not regress prior behavior.

## Implementation Decisions

### Ownership and routing

- The current backend still selects its existing single canvas. This feature
  adds no public canvas-selection or multi-window surface. Refuse a concurrent
  live registration on that same actual DOM canvas, using the existing fallible
  open-window interface; leave its original owner operational. Close then open
  admits a fresh registration.
- Decide ownership by actual node/registration identity, never DOM ID text,
  window labels, broad ancestry or an editable-element whitelist.
- Prefer keyboard listeners at the owned canvas, with source target identity
  and effective focus checked before reading keyboard getters. Moving that
  canvas into a shadow root must not convert host nodes into owned inputs.
- Admit each DOM key state independently. An Up targeted at host HTML after
  native Tab belongs there. Do not fabricate an Up or redirect it to the former
  owner. Reentry cannot turn a later foreign event into owned input.
- The Web backend has no owned IME DOM node today. Do not silently adopt a host
  textarea or promise browser IME by adding a focus-domain abstraction. A future
  editing bridge must register its own identity and lifetime explicitly.

### Dispatch and focus

Keep the existing result type and preserve the two independent observations:

| propagate | default_prevented | DOM consequence |
|---|---|---|
| true | false | Continue propagation; preserve browser default |
| true | true | Continue propagation; cancel a cancelable default |
| false | false | Stop propagation to ancestors; preserve default |
| false | true | Stop ancestor propagation; cancel a cancelable default |

- Use stopPropagation, not stopImmediatePropagation. Same-target observers
  remain eligible. Use nonpassive keyboard registrations. Pre-cancelled events
  stay cancelled; noncancelable events cannot become cancelled.
- Apply conservative DEFERRED/queued outcomes immediately to the originating
  DOM event. A later callback outcome cannot retroactively undo a browser action.
- Effective native focus requires a focused document and the owned canvas
  focus area, including its actual shadow root. Initial state derives from
  those observations. Canvas and browser focus signals update one authority;
  duplicate transitions do not produce contradictory notifications.
- Commit each delivered focus observation before its application callback.
  Preserve causal reentry and converge to the latest observation without guards
  held across user code. Key admission uses DOM ownership, not a stale cache.
- Register application observers before sampling the initial native window
  snapshot. Web bootstrap uses the existing addressed observation path rather
  than unconditional Resumed. Preserve the existing independent execution,
  visibility, focus and retained logical focus policies; an unfocused visible
  canvas must still follow established rendering eligibility.
- Verify existing consumers' recovery after focus loss and return. Do not add
  a new pressed-key service without a reproduced missing obligation.

### Lifetime and failures

- One private window-owned registration aggregate owns immutable Fn listener
  tokens and optional ResizeObserver. No new public listener manager, alternate
  callback FIFO, page-global Rust registry or generic subscription framework.
- Store exact unregister identities and disconnect the observer explicitly.
  Required installation is fallible and transactional. Roll back all installed
  registrations before reporting failure or publishing Created. Preserve the
  existing explicit ResizeObserver-unavailable/window-resize fallback.
- Close fences new DOM admission immediately, then preserves the existing
  causal callback/close ordering for already admitted work. Currently executing
  closures can return safely; captures retire after the required prefix finishes.
  Final owner disposal performs idempotent cleanup; explicit close breaks
  callback/window cycles. Arbitrary strong-owner cycles are not garbage collected.
- Retire keyboard, pointer, wheel, context-menu, focus/hover, resize and observer
  sources together. Clear active capture admission before release. Retire the
  closed registration's platform frame-callback slot by identity; it cannot
  remove a newer slot or emit redraw requests for the retired window.
- Retain the embedder's canvas. Do not change independent host RAF pacing,
  browser-tab lifetime, or the established windowless-owner policy here.
- Move outgoing tokens/captures out before invoking removal, diagnostics or
  destructors. Reuse existing per-capture containment and preserve the first
  failure. If a callback unwind discards a queued clear, terminal registration
  cleanup must still finish rather than relying solely on that queue item.
- Under an exceptional failed DOM detach, fence all delivery, attempt remaining
  retirement, release user captures and keep any still-referenced JavaScript
  function safely inert using its managed lifetime. Report through existing
  contained diagnostics. This exceptional path cannot promise successful
  removal against arbitrary replaced host methods; healthy teardown must not
  use page-lifetime retention. Default wasm panic-abort is not unwind recovery.
- Closed registrations cannot be revived by late callback replacement, resize,
  capture loss, or a stale frame source. Newly offered captures on a retained
  closed handle retire without delivery and outside guards.

### Alternatives

| Shape | Consequence |
|---|---|
| Global listener plus cached focus filter | Leaves foreign ownership ambiguous, cannot stop propagation before ancestors, and retains current lifetime debt |
| Existing generic gloo wrapper | Useful RAII idea, but its FnMut callback and throwing add/remove path conflict with already-proven reentry and fallible installation |
| Private owned Fn registrations | Recommended: one place owns admission, exact detach, observer lifetime and rollback while retaining the current public seam |

Record durable Web mapping/ownership decisions in crate architecture. Use the
existing accepted input, lifecycle and terminal-close decisions; amend or
supersede an ADR explicitly if implementation needs a changed cross-crate rule.

## Testing Decisions

Use the already-approved real DOM producer/public-window seam plus mounted
Focus/Shortcuts consumers. Extend the existing browser probe instead of adding
a parser-only test, mocked event router or new library test API.

- Live retained owners: initial state, canvas↔input/textarea/contenteditable,
  callback-entry getters, exact key counts and healthy return.
- Trusted browser typing and Tab/Shift-Tab: actual text, DOM focus, prevented
  versus allowed traversal, Down/Up targets, next healthy shortcut.
- Synthetic table: all dispatch bits in both states, same-target/ancestor
  observers, already-prevented/noncancelable sources, refused host input even
  when the FLUI callback would prevent everything.
- Shadow embedding: registered canvas moved into open/closed root, unrelated
  shadow input; native source identities determine the result.
- Reentry: key getter, nested input, opposing focus transitions, replacement
  and close from input/focus/resize; every accepted event has the documented
  order and retirement does not erase a newer owner.
- Teardown: capture Weak witnesses, repeated close, final-owner Drop,
  retained DOM, queued observer/resize/pointer packets, close/reopen once,
  callback installed after close and a healthy replacement window.
- Installation failures: first and later mandatory listener failures,
  unavailable observer, failed fallback; no Created/partial owner and a
  successful following open. Pair competing failure/retirement points.
- Portable failure seams: single/competing callback and capture unwind plus
  next operation. Browser JS failures and default wasm abort are reported
  separately; they do not prove Rust panic unwinding in Chrome.
- Startup consumer: existing owner-install/observation harness proves actual
  initial false focus and restoration without erasing logical primary focus;
  source findings become repair claims only after public RED.
- Migrate existing browser probe lifetime and keyboard targets; rerun its
  Unicode metadata, getter-reentry and pointer/wheel controls as regression
  checks for this change, without reopening their earlier audits.
- Every behavior repair preserves baseline RED and a production-hunk-reverted
  failure. Restore exact bytes before rebuilding. Final scope gate, independent
  standards/spec review and CI precede an authorized merge.

A headless tab-switch attempt did not make document.hasFocus false; it is
excluded from page-deactivation RED. Controlled focus-source/protocol cases can
check that branch, but real document/system focus transitions need a browser
environment producing the actual source change. No physical keyboard/layout,
native IME, Safari or Firefox execution is claimed by Chrome constructor tests.

## Out of Scope

AltGraph/lock producer policy and Winit modifier sequencing; real Web editing/
IME transport; a new multi-window/canvas-selection API; pointer/wheel default-
action policy changes; host RAF/idle pacing redesign; unrelated ADR hygiene.
These remain recorded follow-ups, not requirements removed from this feature.

## Further Notes

The owner approved the contract and implementation on 2026-10-12. Archive its
requirements/design/tasks and publish one feature spec with ready-for-agent. The single implementation PR closes #1049 and
#1050 only after their producer and production-consumer acceptance is complete.

Sources: [UI Events WD 2026-02-21](https://www.w3.org/TR/2026/WD-uievents-20260221/#events-keyboard-event-order),
[DOM](https://dom.spec.whatwg.org/#concept-event-dispatch),
[HTML focus](https://html.spec.whatwg.org/multipage/interaction.html#focus),
[ResizeObserver disconnect](https://drafts.csswg.org/resize-observer/#dom-resizeobserver-disconnect),
locked [wasm-bindgen closure implementation](https://github.com/wasm-bindgen/wasm-bindgen/blob/a579ee62b631fd1fa96c7740b52a2952b07c0219/src/closure.rs),
and [gloo-events implementation](https://github.com/rustwasm/gloo/blob/07345515229f46045cfa4f480ae401a464a4569a/crates/events/src/lib.rs). UI Events is a Working Draft, not a Recommendation.
