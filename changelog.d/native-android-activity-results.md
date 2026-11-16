### Added

- A capability provider reaches the running Android Activity through `ProviderContext::activity` (`None` on every other platform) and starts an activity for a result with `Activity::launch_for_result`: it fills an `Intent` the host made with its own `jni` crate, and hears the answer through a `Completion` that ends exactly once, with an `ActivityResult` or a `ResultRefused` (too many open, no Intent made, not launched, cancelled, the Activity ended). The answered `Intent` stays with the host; the provider reads it once with `Activity::take_data`. Each request carries a `ResultToken` minted per call; `Activity::cancel` ends one and finishes the activity it started.
- An answer that reaches a recreated Activity, or a process Android restored after killing it behind another app, waits for `Activity::reattach` under the request's `ResultKey`, which the Activity's saved state carries.
- `CapabilityRegistryBuilder::host` and `CapabilityRegistry::host`: the `HostBridge` through which the composition root offers what each run's host has beyond the window.

### Changed

- `ProviderContext::new` takes the `HostBridge` its providers read.
- `flui_native::android_platform` returns the platform together with the Activity it runs in, which the composition root offers capability providers.
- On Android 11 (API 30) and later the window no longer resizes when the soft keyboard shows or hides (`adjustNothing`); the keyboard reaches layout as its inset, as before. Below API 30 the window keeps `adjustResize`, the only mode in which Android reports the keyboard's inset there.
- The protocol between the native library and FLUI's Java classes is 4: an APK must carry the `classes.dex` of the same `flui-native`, which `flui build` packs.
