### Added

- A capability provider reaches the running Android Activity through `ProviderContext::activity` (`None` on every other platform) and starts an activity for a result with `Activity::launch_for_result`: it fills an `Intent` the host made with its own `jni` crate, and hears the answer through a `Completion` that ends exactly once, with an `ActivityResult` or a `ResultRefused` (too many open, no Intent made, not launched, the host could not hand it to Android, cancelled, nothing pending, the Activity ended). The answered `Intent` stays with the host until the provider reads it once with `Activity::take_data`. Each request carries a `ResultToken` minted per call; `Activity::cancel` ends one and finishes the activity it started.
- Requests are keyed under the capability that acquired the Activity (`RequestKey`), so two capabilities never receive each other's answers. An answer that reaches a recreated Activity, or a process Android restored after killing it behind another app, waits for `Activity::reattach` under the request's `ResultKey`, which answers `NothingPending` when there is nothing to hear.
- `flui-sdk`'s `platform::capability` re-exports what a provider of activity results writes (`Activity`, `ResultKey`, `ResultRequest`, `ResultOutcome`, `ResultRefused`, `ActivityResult`, `ResultCode`, `ResultToken`, `JniFrame`, `IntentFrame`, `InvalidResultKey`).
- `CapabilityRegistryBuilder::host` and `CapabilityRegistry::host`: the `HostBridge` through which the composition root offers what each run's host has beyond the window.

### Changed

- `ProviderContext::new` takes the `HostBridge` its providers read and the name of the capability being provided.
- `flui_native::android_platform` returns the platform together with the bridge of the Activity it runs in, which the composition root offers capability providers.
- On Android the window keeps one soft input mode on every API level: `adjustResize` with the window edge to edge, so the soft keyboard reaches layout as its inset and the surface keeps its size.
- The request codes `0xFF00` to `0xFFFF` are FLUI's on Android: an application that subclasses `FluiActivity` and starts activities for results itself uses codes below them.
- The protocol between the native library and FLUI's Java classes is 4: an APK must carry the `classes.dex` of the same `flui-native`, which `flui build` packs.

### Fixed

- On Android an Activity callback queued for the native side is no longer lost when reading it fails: the queue keeps it until it was read whole.
