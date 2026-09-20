# ADR-0073: UIKit scenes own native attachments, sessions retain UI state

Status: accepted and implemented; scoped native protocol acceptance and independent reviews passed.

## Context

`UIApplicationDelegate` is a process bootstrap, not a window lifetime. Running the
entire app bootstrap from a second scene connection would restart services and
replace retained widget state. Conversely, keeping a fixed `UIWindow` would leave
native ownership detached from UIKit's scene. A renderer can retain a raw-window
handle owner after its surface is released, so replacing the underlying `UIView`
would violate the borrowed handle's non-dangling-pointer contract.

## Decision

The application delegate runs `on_ready` once. A registered, fallible scene
consumer receives the session identity, attachment identity and exact logical
window on connection. Registration precedes scene admission. The runner owns
process services, watcher and root configuration; each accepted session owns a
realm/tree/render lane. Disconnect retains that realm and reports reversible
`Detached`. Reconnect attaches the same logical window and retained tree to a
new native container. Terminal discard/close disposes the realm; a subsequent
session installs a fresh tree from the retained root configuration. Explicit close
and OS discard retain terminal persistent identities for the process lifetime;
a delayed reconnect cannot resurrect them even if UIKit refuses destruction.
Failed initial installation instead emits `InstallationAborted`, disposes its
provisional resources, and permits retry of that same session. Terminal intent
already accepted during installation takes precedence over rollback.

The initial implementation admits one logical session at a time. Other sessions
are rejected before constructing their UI; this is not a claim of complete
multiwindow support. Native window publication follows successful consumer
installation and another admission check. Close, discard and quit during setup
cancel publication. Provisional registered realms are cleaned up on failure.

A stable `FluiView` belongs to the logical session. `UIWindow` and its controller
belong to an attachment. Disconnect releases the GPU surface before detaching
native containers and invalidates the display link. New raw handles are rejected
while detached, closed, or off the owner thread; already borrowed handles retain
their stable view through their owner. Scene callbacks validate the originating
scene and attachment. Frame callbacks validate the actual display link; touch
callbacks validate the originating native window.

Native values use `dispatch2::MainThreadBound`, with cached plain metrics for
cross-thread reads. A worker's last logical-window drop queues native retirement
on main rather than synchronously waiting for main. This permits main to join
that worker without a destructor deadlock. The owner queue must remain serviceable
for deferred native release.

Scene dispatch leases its consumer outside the registry borrow and serializes
attachment ownership changes. Execution observations continue through the existing
window lifecycle pump so newer resource intent can supersede an older transition.
The ownership drain encloses an outer execution observation. A reentrant
disconnect fences its attachment immediately and pauses its physical display link;
container retirement waits until the active lifecycle pump returns. Old-origin
observations cannot supersede the subsequent Detached/surface-release sequence.
The surface-release observer still sees the native attachment. Reconnection clears
the fence only for the newly admitted attachment.
Process owner turns use the existing typed
`OwnerSignal` through GCD, independently of windows and display links. Quit fences
scene and proxy admission and tears down framework resources; it does not force
process exit or promise that `UIApplicationMain` returns. Ordinary callback panics
are contained at the Objective-C boundary and cleanup continues. A `FnOnce` body
and its capture destructor both panicking during the same unwind remains Rust's
standard process-abort case. Independently stored callback captures retire in
separate containment boundaries, outside every slot lock. The asynchronous UIKit
destruction-error block owns its own boundary; synchronous scene dispatch cannot
protect a later invocation by UIKit.

## Mapping decisions

Apple's process/session/attachment distinction owns native lifetime. FLUI retains
its Rust realm and three-tree across a reversible disconnect rather than rerunning
the process bootstrap. The execution/focus/visibility protocol remains ADR-0072;
scene ownership introduces no second lifecycle reducer. `Running` means OS
execution eligibility, not a successful GPU surface restoration. An existing
surface restoration failure stays non-renderable and currently needs another
availability edge to retry; automatic restoration retry is a separate obligation.

## Verification and limits

The native protocol probe calls only selectors supported by the actual owned
UIKit delegate. It distinguishes protocol delivery from real OS scene reclaim.
The app fixture uses the sole `flui` facade for widgets/services and native SDK
bindings solely to drive the protocol. GPU evidence comes from the existing
`flui.gpu` event emitted after submission and presentation. The worker-drop probe
uses a weak UIView witness and joins the last-owner worker from main.

Verified SDK/toolchain details and final case results belong in the task's
acceptance evidence. This migration does not implement safe-area layout, OS
background execution grants, unrestricted multiple-scene UI, or guarantee final
termination callbacks after an OS kill. It does not claim validation on SDK 27.

## Sources

- [Apple TN3187: Migrating to the UIKit scene-based life cycle](https://developer.apple.com/documentation/technotes/tn3187-migrating-to-the-uikit-scene-based-life-cycle), revision 2026-03-16, checked during design.
- [Flutter UIScene migration](https://docs.flutter.dev/release/breaking-changes/uiscenedelegate), checked during design; FLUI does not copy its runtime topology.
- Installed `dispatch2` 0.3.1 `MainThreadBound` and `objc2-ui-kit` 0.3.2 sources; Xcode 26.2 SDK.
- `raw-window-handle` 0.6.2 borrowed handle validity contract.
