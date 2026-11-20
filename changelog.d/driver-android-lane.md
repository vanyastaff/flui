### Added

- `flui_driver::lane::android`: the Android lane, which drives a FLUI app on an emulator or a device from any host through `adb`. A contact is held through `input motionevent` (touch, the mouse's primary button), keys are sent at their release as `keyevent` or `keycombination`, printable ASCII is typed with `input text`, a scroll goes through `input mouse scroll`, and a pen goes through a `uinput` device the lane registers where the device lets the shell (`uinput_device`, probed once a session; the pen is refused elsewhere). It refuses a second contact, the keys and chords that leave the app or change the device (home, recents, back, power, volume, the shade, Meta's shortcuts) and a contact going down at the display's edge, checks that the app's window holds the focus before each press, key and text, guards a device by watching `getevent` and runs an emulator unguarded. Positions are order-only. `AndroidLane::start` installs and starts an app with its endpoint and token as intent extras and forwards a loopback port to it; `lane::android::launch` attaches a driver to it; `lane::android::host_device` takes only the device `FLUI_ANDROID_SERIAL` names. `capture_window` reads `screencap`, cut to the app's window.
- `flui_driver::testing::ScriptedAdb`: a device scripted for the Android lane, so its commands, refusals and releases run without a device.

### Changed

- `lane::send_guarded` answers `Error::ReleaseFailed` when the release that ends a run leaves input held, also after the run failed or was interrupted: the message names the run's own failure as well. Before, the run's failure hid what stayed held.
- `os_input::admit` refuses OS input on a physical device (`Isolation::Device`) without a user-activity guard, as on the shared desktop.
- Launching an `Artifact::Apk` through `Driver::launch` answers `not_supported` naming `android_launch`: an APK is started through `lane::android::launch`, which names its package and activity.
