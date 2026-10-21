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

**System preference**:
A user or operating-system preference that can affect several presentations,
independently of whether any native window currently exists. A presentation's
geometry and appearance remain its own properties.
_Avoid_: Window state, application policy

**Application policy**:
An application's choice of how to respond to a system preference. It can honor
or override that preference without changing the operating system's setting.
_Avoid_: System preference

**Preferred UI languages**:
The ordered languages the user requests for interface content, before an
application selects among the resources it supports.
_Avoid_: Formatting region, resolved locale

**Resolved locale**:
The language identity selected for an application's available resources after
applying its explicit choice or preferred UI languages and fallback policy.
_Avoid_: System preference, formatting region

**Frame demand**:
A pending request for a future UI update. It is distinct from the host's
notification that execution may proceed and from a frame already in progress.
_Avoid_: Frame wake, frame execution

**Native host**:
FLUI's own integration with one operating environment (Windows, macOS, iOS,
Android, the Web, Linux), written against that environment's native APIs; in
`flui-native` it is one table of types and one file per feature.
_Avoid_: Backend, port, platform, shell

**Headless host**:
A host with no operating environment behind it, driven by tests and
automation. It is not a native host and never stands in for one.
_Avoid_: Mock platform, fake host, headless backend

**Ready platform**:
An operating environment whose native host provides every core integration
and has passed its conformance checks in an executed run there. Compiling for
an environment does not make it ready.
_Avoid_: Supported platform, experimental platform

**Capability**:
An operating-system service outside the core integrations, reached by a widget
through a typed handle and provided by a separately versioned crate. Its absence
on an environment is a value, never an error the widget cannot inspect.
_Avoid_: Plugin, platform feature

**Owner retirement**:
The permanent end of a UI execution owner's task and callback lifetime.
Closing one presentation does not by itself retire its runtime's owner.
_Avoid_: Presentation closure, native window closure, pause

**Semantic region**:
An independently described accessible part of custom content, with its own
bounds and actions. Several regions may belong to one visual element.
_Avoid_: Compositing layer, render object
