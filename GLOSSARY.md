# FLUI UI ownership

FLUI separates an independent UI runtime from its presentations and the host
that provides execution and presentation facilities.

## Language

**UI runtime**:
An independent instance of UI state and execution, with one owner and one or
more presentations. Separate runtimes remain independent even on the same host.
_Avoid_: Realm, UI session, UI host, window

**Host**:
The environment that supplies execution and presentation facilities to UI
runtimes. A host may serve several runtimes without merging their state.
_Avoid_: Realm, UI runtime

**Presentation**:
One presentation of a UI runtime, with its own lifetime. A runtime can have
several presentations, including presentations without a native window.
_Avoid_: Session, host, native window

**Native window**:
A platform window used to present UI. Its lifetime and identity are distinct
from those of the UI runtime and presentation it serves.
_Avoid_: Presentation, session
