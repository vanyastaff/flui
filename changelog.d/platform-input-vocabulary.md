### Added

- A scroll event says whether the platform reversed the device's direction (natural scrolling): `ScrollEvent::direction` is a `ScrollDirection`, read on macOS, in Safari and on Wayland and `Unknown` where the platform does not say, so a slider or stepper driven by the wheel can move with the fingers.
- A dead key carries the accent it leaves pending: `Key::Dead(Option<char>)`, filled on Windows, Linux and Android.

### Changed

- A key event's state says what kind of key-down it is: `KeyState::Down(KeyPress::First | KeyPress::AutoRepeat | KeyPress::Unpaired)` or `KeyState::Up`, and `KeyEvent::state` is a public field. A release can no longer be built as a repeat. `KeyRepeat`, `KeyRelease`, `KeyEvent::repeat`, `KeyEvent::release`, `with_repeat` and `with_release` are removed; match `KeyState::Down(_)` to ask whether a key is down.
- A dead key is no longer `Key::Named(NamedKey::Dead)`; `NamedKey` has no `Dead` variant.
- Input recordings are format version 4 (a key's state is `down`, `repeat`, `unpaired` or `up`, and a key may be `dead`); version 3 recordings are refused. The agent protocol is 0.16, which a 0.15 client cannot speak to.
- `KeyEventBuilder` (testing) takes the repeat in its state and loses `repeat`.

### Removed

- `PointerEvent::Revise`, `PointerRevision`, `PointerEvent::PenSqueeze`, `PenSqueeze`, `PenSqueezePhase` and `Listener::on_pointer_revise`: no platform produced them. They return with the host that reports them.

### Fixed

- Android: a hardware dead key composes with the next key (`´` then `e` types `é`); before, the accent was dropped and the letter typed bare.
