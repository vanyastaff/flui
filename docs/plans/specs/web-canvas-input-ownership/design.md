# Web canvas input ownership

Status: Approved, 2026-10-12; implemented and reviewed.
Release requires exact-head CI and owner authorization to merge.
Specification: [#1560](https://github.com/vanyastaff/flui/issues/1560).
The approved requirements are authoritative; this document records the design.

## Boundaries

The Web host owns DOM registration and translation. The application installs
observers and submits the existing addressed native snapshot. The UI runtime
continues to own logical focus and shortcut delivery. Foreign DOM input is
rejected before translation, rather than being filtered by logical widget focus.

One private registration aggregate owns its exact canvas identity, immutable
listener functions, optional resize observer and delivery fence. Opening is a
transaction: successful registration precedes Created; failure rolls back the
installed prefix. Another live registration on the actual canvas is refused
without displacing its owner. The public window interface remains unchanged.

## Keyboard and focus

Canvas listeners admit each key state from that canvas's effective focus area.
Check document focus, actual root focus and original source identity before
reading keyboard properties. Moving the canvas into shadow DOM preserves its
identity. A host input, including a shadow input, is never an implicit owner.

Apply the existing dispatch result synchronously: stop ancestor propagation
when propagate is false and prevent a cancelable default when default_prevented
is true. Same-target listeners remain eligible. Deferred results retain their
existing conservative meaning. A Tab Up targeted outside the canvas is excluded.

Initial focus is observed rather than assumed. Commit changed focus before
notifying user code, suppress duplicate observations and preserve causal reentry.
Application startup registers observers before sampling execution, focus and
visibility together. Native focus loss retains logical primary focus according
to the existing runtime policy; restoration must resume healthy input.

## Retirement and recovery

Close immediately fences new DOM admission, then preserves the already-admitted
callback prefix and existing close order. Remove owned listeners, disconnect the
observer and clear pointer admission before releasing captures. Take outgoing
ownership out of guards before calling user code or destructors. Final owner
disposal performs the same idempotent retirement; explicit close breaks callback
cycles. Keep the embedder's DOM canvas.

Retire the platform's matching callback slot by identity. A retired registration
cannot erase a newer owner or emit stale redraw requests. Do not redesign host
RAF pacing or introduce a page-global Rust registry or second callback FIFO.

Queued clear alone is insufficient if an admitted callback unwinds. Terminal
cleanup must complete through the existing containment policy, with individual
capture retirement and first failure preservation. Exceptional DOM detach failure
fences delivery, attempts all remaining cleanup, retires application captures and
keeps any still-referenced JS function safely inert through managed lifetime.
Healthy teardown does not retain functions for the life of the page. Late callback
installation on a closed handle cannot deliver or revive the registration.

## Alternatives and evidence

Global listeners plus cached focus preserve ambiguous ownership and listener debt.
A generic gloo listener uses FnMut and throwing add/remove paths, conflicting with
proven getter reentry and transactional installation. Private owned Fn tokens
retain the current public seam without a general subscription framework.

The existing browser probe must retain its real window/platform owner and target
the focused canvas. Preserve Unicode, pointer/wheel and immutable getter-reentry
controls. Public owner/mounted tests exercise startup and logical focus recovery;
portable failure seams exercise Rust unwind separately from browser JS failures.
Each repaired behavior requires baseline RED and production-reverted evidence.

Headless tab switching did not change document.hasFocus and is excluded as live
page-deactivation evidence. Controlled source cases may cover that branch while
reporting the execution limit. Physical keyboard layouts, OS IME and other browser
engines remain unexecuted. Broader input and architecture follow-ups are retained
outside this feature's scope.
