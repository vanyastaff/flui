### Added

- A capability provider reaches the running Android Activity through `ProviderContext::activity` (`None` on every other platform) and starts an activity for a result with `Activity::launch_for_result`: it fills an `Intent` the host made with its own `jni` crate, and hears the answer through a `Completion` that ends exactly once, with an `ActivityResult` or a `ResultRefused` (too many open, no Intent made, not launched, the host could not hand it to Android, cancelled, nothing pending, the Activity ended). The answered `Intent` stays with the host until the provider reads it once by handing the answer's `ResultData` to `Activity::take_data`; a `ResultData` dropped unread releases it. Each request carries a `ResultToken` minted per call; `Activity::cancel` ends one and finishes the activity it started.
- Requests are keyed under the capability that acquired the Activity and the provider's key, kept apart (`RequestKey`), so two capabilities never receive each other's answers, and one capability holds at most `MAX_OPEN_REQUESTS_PER_CAPABILITY` of them. An answer that reaches a recreated Activity, or a process Android restored after killing it behind another app, waits for `Activity::reattach` under the request's `ResultKey`, which answers `NothingPending` when there is nothing to hear.
- `flui-sdk`'s `platform::capability` re-exports what a provider of activity results writes (`Activity`, `ResultKey`, `ResultRequest`, `ResultTicket`, `ResultOutcome`, `ResultRefused`, `ActivityResult`, `ResultData`, `ResultCode`, `ResultToken`, `JniFrame`, `IntentFrame`, `InvalidResultKey`).
- A request lasts while its provider holds its `ResultTicket` (`ResultRequest::into_parts` hands it out): dropping it ends the request as a cancel does, so a provider or runtime that went stops waiting and frees its place. An answer whose `Intent` the host could not keep ends as `ResultRefused::DataLost` with its code, and one the host cannot read as `ResultRefused::AnswerUnreadable`. An earlier Activity's requests still count for their capability after a recreation.
- `CapabilityRegistryBuilder::host` and `CapabilityRegistry::host`: the `HostBridge` through which the composition root offers what each run's host has beyond the window.

### Changed

- A capability's `NAME` must be a valid `ResultKey` (1 to 64 bytes of printable ASCII, no spaces); the registry refuses another with `CapabilityConflict::InvalidName`.
- `ProviderContext::new` takes the `HostBridge` its providers read and the name of the capability being provided.
- `flui_native::android_platform` returns an `AndroidStart`: the platform and the bridge of the Activity it runs in, which the composition root offers capability providers.
- On Android the window keeps one soft input mode on every API level: `adjustResize` with the window edge to edge, so the soft keyboard reaches layout as its inset and the surface keeps its size.
- The request codes `0xFF00` to `0xFFFF` are FLUI's on Android: an application that subclasses `FluiActivity` and starts activities for results itself uses codes below them.
- The protocol between the native library and FLUI's Java classes is 4: an APK must carry the `classes.dex` of the same `flui-native`, which `flui build` packs.

### Fixed

- On Android an Activity callback queued for the native side is no longer lost when reading it fails: the queue keeps it until it was read, and a record that cannot be read is skipped instead of holding the queue.
- On Android a panic in work FLUI posts to the Java main thread is contained there instead of aborting the process.
