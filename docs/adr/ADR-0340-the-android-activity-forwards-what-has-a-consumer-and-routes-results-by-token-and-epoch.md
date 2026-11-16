# ADR-0340: The Android Activity forwards what has a consumer, routes results by token and epoch, and keeps one soft-input mode

- **Status:** Accepted (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) decision 2 in its list of what
  `FluiActivity` forwards (`onMultiWindowModeChanged`, `onTopResumedActivityChanged`,
  `onUserLeaveHint` and `onRequestPermissionsResult` are not forwarded until a consumer needs
  them); decision 8 in how `launch_for_result` is shaped (the queue, the buffering and the bridge
  through the provider context stand); and the sentence of decision 6 and the row of its
  Confirmation table that leave `windowSoftInputMode` undecided.
- **Related:** [ADR-0154](ADR-0154-capability-crates.md) §9 (the host bridge through the provider
  context), [ADR-0152](ADR-0152-capability-seam-revised.md) §2 items 5, 6 and 8 (`Completion`,
  the provider context, the permission vocabulary), [ADR-0277](ADR-0277-a-window-publishes-its-insets-and-the-keyboards-motion.md)
  (the keyboard inset and its motion), [ADR-0349](ADR-0349-android-text-input-loses-no-edit-and-restarts-the-keyboard-only-for-its-composition.md) §2 (the Java
  side never waits for the owner)
- **Sources:** flutter/flutter@f60eef8885
  `engine/src/flutter/shell/platform/android/io/flutter/plugin/editing/TextInputPlugin.java`,
  `ImeSyncDeferringInsetsCallback.java`, and
  `packages/flutter_tools/templates/app/android.tmpl/app/src/main/AndroidManifest.xml.tmpl:14`

## Context and problem statement

`FluiActivity` (ADR-0255 decision 1) forwards Activity callbacks into a queue in `FluiHost`. ADR-0255
decision 2 lists seven callbacks to forward; four of them have no consumer in the framework, and a
forwarded callback nobody reads is an unconnected surface. Decision 8 asks for `launch_for_result`
routed "by a token per call" without saying how a token survives an Activity Android replaces, how
an answer meant for an earlier Activity is told apart, or what bounds the requests. Decision 6
leaves `windowSoftInputMode` undecided, and `NativeActivity.onCreate` sets `adjustResize` on the
window itself, so a manifest attribute decides nothing. The question: what does the Activity
forward, how does a capability provider start an activity for a result and hear it whatever
happens to the Activity, and in which soft-input mode does the window run?

## Decision drivers

- No inert surface: a forwarded callback, a host-table position or a public type arrives with its
  consumer.
- An answer is matched to its request by typed identity, never by a number another request could
  reuse (`AGENTS.md`, "Recurring defects").
- Accepted work is delivered or refused with a typed error on every exit: answer, refusal by
  Android, a host failure, cancel, the Activity's end.
- Every queue and table is bounded; an `Intent` from another application is untrusted input.
- The contract crate holds no JNI type and no `unsafe`; each capability crate brings its own `jni`.
- The keyboard reaches layout as an inset (ADR-0277), and the surface does not resize for it, on
  every API level from the floor.

## Considered options

1. Forward every callback ADR-0255 lists; results routed by a request code the host picks per
   call; the soft-input mode switched at API 30 (`adjustNothing` above it).
2. Forward only what has a consumer; results routed by a per-call token and the Activity's epoch,
   answers of an earlier Activity kept for a reattachment by key; one soft-input mode on every
   level, `adjustResize` with the window edge to edge.
3. Leave results to each capability crate (its own `Fragment` or registry), as Flutter's plugins do
   with their own request codes.

## Decision

Chosen option: 2.

§1. **What the Activity forwards.** `onTrimMemory`, the intent of `onCreate` and `onNewIntent`, and
`onActivityResult`, with `onStart`, `onStop`, `onDestroy` and `onSaveInstanceState` for the result
bridge. Not forwarded, each until its consumer: `onMultiWindowModeChanged` (an adaptive-layout
feature that reads whether the window shares the screen), `onTopResumedActivityChanged` (window
focus in multi-resume, for the window-activation facts of the snapshot), `onUserLeaveHint`
(picture-in-picture entry), `onRequestPermissionsResult` (the first capability with an OS
permission, ADR-0152 §2 item 8). A permission request joins the result bridge below as one more
`FluiHost` method and answer kind; no second mechanism.

§2. **The result bridge.** `ProviderContext::activity()` hands a provider an `Activity` scoped by
the `CapabilityId` its registration minted (`CapabilityRegistrar::provide`): a name and an
identity compared by address, which only the provider's `RegisteredProvider` puts in the context,
so no caller names another capability by its string. The capability's name, which the registry
holds unique and to the `ResultKey` rule, keys requests across Activities. The `Activity` is `None`
on every host but Android's, and the host table and `Platform` gain nothing: `android_platform`
returns an `AndroidStart` with the platform and the Activity's bridge, which every Android
platform has (one whose epoch cannot be read does not start). The composition root installs the
bridge in a `HostBridge` only it holds, which hands the bridge its `ResultMint`; a
`ResultTicket`, a `ResultData`, an `ActivityResult` and a `ResultRequest` are made only with it,
so a provider holds only what the host handed it. `Activity::launch_for_result(completions, key,
fill)` lends the provider an empty `Intent` the host made (`IntentFrame`), so no reference flows
from a provider's JNI code into the host, and answers with a
`Completion<Result<ActivityResult, ResultRefused>>`. A request is a `ResultToken` minted per call
(`ResultTokenMint` over `IdMint`, so a token is also its request's age) and the Activity's epoch,
which `FluiHost.nextEpoch` gives each platform of the process; its key is a `RequestKey`, the
capability's name and the provider's key, kept and compared apart in Rust, Java and the saved
state. Android sees a request code from the range `FluiHost` keeps for the host
(`FIRST_HOST_CODE..=LAST_HOST_CODE`, within 16 bits), so an application's own codes pass through
untouched. The started requests' codes and key parts are saved with the Activity's state and
restored before `NativeActivity.onCreate` starts the library.

An answer of the Activity's own epoch settles its token's request once. An answer of another epoch
(epoch 0 for a process restored after death) under a readable key, whatever it says (a result, a
refusal, an unreadable record), ends the oldest reattachment still waiting under that key, oldest
by token, or waits for one, oldest first by its arrival sequence. How many answers are still to
come under a key has one definition, `FluiHost.stillToCome`: the earlier requests under it waiting
to start, started, or answered and not drained. A reattachment opens only while that count exceeds
the reattachments already waiting, and each turn the host holds the waiting ones to it, ending the
youngest beyond it as `NothingPending`, so none waits for an answer that will not come.

A request lasts while its provider holds its `ResultTicket`; dropping it is the one way to cancel.
The drop records the release and wakes the owner, and nothing more; the owner's turn ends the
request as `Cancelled` and asks Java to cancel a started activity, which takes it off Java's open
requests at once (its code parked until Android answers for it). The contract's `Resolver` answers
no liveness query. Every request ends exactly once: the answer, `NotLaunched`, `TooManyOpen`,
`HandOffFailed` (a failed or panicked hand-off to Android's main thread, or one the main thread has
not taken within a deadline the loop waits for), `Cancelled`, `ActivityEnded`, `NothingPending`,
`DataLost` (an `Intent` the host could not keep, with the answer's code) or `AnswerUnreadable` (a
record the library cannot read); every record the host takes off Java's queue settles a request or
is reported. The answered `Intent` stays with the host on the owner thread, under its request's
token and capability, until that capability's provider hands the answer's `ResultData` to
`Activity::take_data` (another capability's is refused as `DataRefused::OtherCapability`); a
`ResultData` dropped unread releases it.

Each count has one owner. `FluiHost` counts the requests it holds, waiting to start, started, or
answered and not drained, of every Activity of the process: at most `MAX_OPEN_REQUESTS` in all and
`MAX_OPEN_REQUESTS_PER_CAPABILITY` of one capability, and a launch past either is answered
`TooManyOpen`. The library counts only what Java no longer holds: the drained `Intent`s nobody took
yet (at most `MAX_OPEN_REQUESTS`, and `MAX_OPEN_REQUESTS_PER_CAPABILITY` of one capability), the
earlier answers waiting for a reattachment (at most `MAX_EARLIER`), and the launches posted to
Android's main thread that Java has not taken yet (at most `MAX_OPEN_REQUESTS`). Nothing accepted is
let go to make room, and one provider cannot keep the others from asking, before or after a
recreation. Java's queues are read before Java lets go of what was read, record by record, each
failed JNI read's exception cleared before the next call, so a failed drain loses nothing and a
record that cannot be read never holds its queue. Every closure the host posts to Android's main
thread goes through one function, `fn post` of `owner::android::main_thread`, which runs it inside
the panic boundary and reports every exit but success; `cargo xtask panic-boundary` refuses the
identifier `run_on_java_main_thread` anywhere else in a build, the tokens of macros included.

§3. **The soft-input mode.** `adjustResize` on every level, with the window edge to edge:
`Window.setDecorFitsSystemWindows(false)` from API 30, the decor view's
`SYSTEM_UI_FLAG_LAYOUT_STABLE | SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN`
below it. `FluiActivity.onCreate` sets it after `NativeActivity.onCreate` and before the window's
first layout; the decision as data is `owner::android::window_setup`, a pure function of the API
level, and a test holds `FluiActivity`'s constants to it. In that state Android dispatches the
input method's inset to the views and leaves the content, and so the surface, at its size.

### Consequences

- Good, because no callback is forwarded and no type published without a reader, and each one not
  forwarded names the feature that brings it.
- Good, because an answer can never settle a later request that reused a number, two capabilities
  can never receive each other's answers, and an answer outlives the Activity and the process.
- Good, because every exit of a request is typed and every table is bounded, an `Intent` is never
  parsed by the host and is released when nobody can read it, and a panic in work the host posts
  to Android's main thread is contained there instead of aborting the process.
- Good, because one soft-input mode holds on the whole supported range, the mode Flutter's
  embedding uses.
- Bad, because the Java and JNI halves are type-checked by CI and run only on a device, and the
  edge-to-edge setup rests on calls Android deprecated (the layout flags below API 30,
  `setDecorFitsSystemWindows` from API 35, where an app targeting it is edge to edge already).
- Neutral, because the reattachment by key is the provider's to call as it starts; one that does
  not hears nothing of an earlier Activity's answer, which is let go when the Activity ends.

### Confirmation

- The router's table runs on every native host beside it (`owner::android::activity_results`):
  `an_answer_settles_the_request_its_token_names_once`,
  `each_launch_that_is_not_answered_ends_with_its_reason`,
  `the_untaken_intents_and_unhanded_launches_are_bounded`, `an_intent_nobody_receives_is_released`,
  `an_earlier_activitys_answer_waits_for_its_key`,
  `a_reattachment_with_nothing_pending_holds_nothing`, `the_end_refuses_every_open_request`,
  `a_reattachment_let_go_leaves_the_answer_to_the_one_still_waiting`,
  `a_ticket_let_go_wakes_the_owner_and_cancels_its_launch`,
  `the_oldest_reattachment_receives_the_answer_whatever_ended_before`,
  `every_earlier_record_ends_a_waiting_reattachment`,
  `every_answer_taken_off_the_queue_ends_its_request`,
  `answers_are_read_as_the_java_classes_lay_them_out`; the drains record by record,
  `a_record_that_cannot_be_read_is_skipped_and_the_rest_delivered`; the posting function's exits,
  `posted_work_reports_every_exit_but_success`; the soft-input table,
  `the_window_resizes_its_insets_not_its_surface_on_every_level` and
  `fluiactivity_applies_this_table`; `cargo xtask panic-boundary` and its self-test for posts to
  the Java main thread.
- `flui-sdk`'s `a_package_provides_activity_results_against_the_sdk_alone` names every type a
  provider writes; `flui-testing`'s `a_provider_sees_the_activity_the_host_offers` runs the seam.
- The device runs are rows of `docs/platform-parity.md`: "Activity results for capability
  providers", "Soft-keyboard insets" (the count of `WindowResized` during a keyboard animation on
  both sides of API 30), "Memory pressure".

## Pros and cons of the options

### 1. Forward everything; request codes as identity; two soft-input modes

- Good, because it follows ADR-0255's list as written.
- Bad, because four forwarded callbacks have no reader.
- Bad, because a request code is a number the next platform of the process, or an application's
  own `startActivityForResult`, can reuse: an answer reaches the wrong request.
- Bad, because a boundary at API 30 splits the supported range in two behaviours, and
  `adjustNothing`'s IME dispatch above it is not stated by the platform documentation.

### 2. Forward what has a consumer; token and epoch; one mode, edge to edge

- Good, for the reasons under Consequences.
- Bad, because the bridge has more parts (epoch, keys, reattachment, held `Intent`s) than a
  request code alone.

### 3. Each capability crate routes its own results

- Good, because the host stays small.
- Bad, because a capability crate cannot subclass the Activity; it would need a headless
  `Fragment` per call (the stopgap ADR-0255 rejects) or a process-global registry (ADR-0097).
- Bad, because two crates choosing one request code collide, as Flutter's plugins can.

## More information

- Android documents `WindowInsets.Type.ime()` and `WindowInsetsAnimation` from API 30, and
  `WindowInsetsCompat` documents that below it the IME inset is reported only while the window's
  soft input mode is `SOFT_INPUT_ADJUST_RESIZE`
  ([WindowInsetsCompat.Type.ime](https://developer.android.com/reference/androidx/core/view/WindowInsetsCompat.Type#ime())).
  `Window.setDecorFitsSystemWindows(false)` hands the insets to the app instead of fitting the
  content ([Window.setDecorFitsSystemWindows](https://developer.android.com/reference/android/view/Window#setDecorFitsSystemWindows(boolean))),
  and the guide to the keyboard's animation pairs it with `adjustResize`
  ([Synchronize animation with the software keyboard](https://developer.android.com/develop/ui/views/layout/sw-keyboard)).
  None of these states that `SOFT_INPUT_ADJUST_NOTHING` dispatches the IME inset.
- Flutter's template keeps `adjustResize`, and its embedding syncs the IME inset from API 30 with
  `ImeSyncDeferringInsetsCallback`.
- Compose's `ActivityResultRegistry` keys a launcher by a saved string and holds a result that
  arrives while nobody is registered; FLUI's reattachment is that idea with the capability's name
  as the namespace and an explicit "nothing pending".
- Revisit when FLUI leaves `NativeActivity` (ADR-0255 decision 9) or when a capability needs an
  OS permission (§1).
