# ADR-0185: Browser host motion uses the existing owner signal

- **Status:** Accepted
- **Date:** 2026-10-09
- **Supersedes:** [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §7's browser registration posture only.
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md),
  [ADR-0184](ADR-0184-presentation-motion-policy.md)

## Decision

The browser host owns one motion observation set before creating a canvas. It
subscribes to `prefers-reduced-motion: reduce` and checks the complementary
`no-preference` query without another listener. A Reduce match observes Reduce;
an explicit no-preference match observes NoPreference. If neither matches, the
feature is unavailable and remains unknown, as demonstrated by
[WebKit's explicit-query example](https://webkit.org/blog-files/prefers-reduced-motion/prm.htm).
These values supply the existing `SystemPreferences` snapshot. Failed query or
listener installation is an error, not an observed default.

Change events invalidate the host through its existing `OwnerSignal`; they do
not invoke runtime code or supply a second policy authority. The native posting
edge uses `js_sys::futures::spawn_local`, which schedules a microtask even for a
ready future. Admission therefore commits before callbacks run. The shared
signal retains coalesced delivery debt and schedules reentrant wakes on another
turn. Wake and quit proxies work without a canvas; deferred window creation
remains unsupported because owner-local creation already returns Ready.

The application runner installs the same host-refresh callback used on desktop.
It checks the installed loop identity and logical host before accepting native
observations. A retained callback from an obsolete host cannot sample or update
its successor. Initial publication still precedes root mount; subsequent updates
reach presentation clocks and inherited policy through ADR-0184.

Quit closes signal admission before removing the media-query listener and
retiring callbacks. The listener captures an admission flag and a weak signal,
never a canvas or runtime. If native listener removal fails, retain its JS
trampoline with admission closed: later events remain safe and cannot wake a
replacement. DOM calls and callback retirement occur outside state locks and
RefCell borrows. A stopped owner refuses preference reads.

## Alternatives

- Sampling from every animation frame would couple settings to canvas lifetime
  and require frame production while idle. It also provides no independent
  owner-delivery contract.
- Calling the runtime directly from the DOM listener would bypass admission,
  replacement identity and ordered owner work. It would duplicate the shared
  signal's reentry and retirement protocol.
- Reusing the owner signal supplies the existing wake/quit contract and keeps
  native observations in the host's single publication path.

## Evidence and limits

`browser_motion_observations_wake_replace_and_retire_their_owner` runs through
the public platform on WASM with a controlled JS `matchMedia` boundary. It checks
initial and unavailable observation, asynchronous notification, reentrant wake, hook replacement,
registration failure and recovery, successful retirement, failed removal with an
inert retained trampoline, and a successor owner unaffected by the old listener.
The unimplemented source fails its initial NoPreference assertion.

This Node-hosted fixture executes the adapter and its JS bindings; it does not
change an operating-system preference or establish an actual browser's OS
notification behavior. Native browser execution and rendered live-update
acceptance remain separate requirements. The existing RAF pacing behavior is
unchanged. WASM panic-abort builds cannot provide Rust unwind recovery; the test
exercises native API errors rather than claiming panic recovery there.
