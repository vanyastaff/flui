# Application runtime architecture

## Mapping decisions

### Native execution caps remain presentation-local

ADR-0072 adds a window execution observation to the existing presentation facts.
Suspension caps only that presentation at Paused; a running sibling can keep the
shared scheduler eligible. Host suspension and terminal close remain stronger than
late local Running/focus/visibility events. Callback registration precedes a batch
snapshot, whose execution/focus/visibility fields are committed together before
reconciliation and public lifecycle notification. Input cancellation uses the same
addressed path as focus loss and runs before lifecycle observers.

The facade continues to expose AppLifecycleState through its existing lifecycle
handle, without adding raw platform control types. A surface restoration failure
stays released and skips GPU work. Existing device recovery does not imply an
automatic surface recreation retry; another availability request is currently
required. Scene migration and background owner waking remain explicit follow-ups.

### UIKit process and session ownership

The UIKit runner starts services, execution pools and its development watcher
once per process. Its private session controller installs a real realm only for
a fresh scene session; reconnect selects the retained realm. Terminal discard
uses the existing addressed close path and removes the session before disposal.
The controller's generic key permits the same production ownership logic to run
with real headless realms in host tests; it is not another lifecycle reducer or a
public raw-platform capability. Root configurations can retain application-owned
state while a new session creates fresh `ViewState`. See ADR-0073 for native
attachment lifetime, panic containment and platform limits.
