### Added

- **The development agent on Android** (`flui-app`): the Android runner drives an installed
  `DevAgentHook` as the desktop and iOS runners do: attached for the loop and handed the window
  once it is installed. An Android app builds its `AgentEndpoint` itself, since no launch
  environment reaches its process.
- **`cargo xtask device android-gallery`**: the interaction gallery on an attached Android device,
  every screen driven by `uinput` touches, stylus, mouse and hardware key events and the device's
  soft keyboard, each outcome read through the development agent and the screen's pixels.

### Fixed

- **Agent rectangles doubled by the device pixel ratio** (`flui-semantics`): a wire read's
  `surface_rect` applied the ratio twice on any window whose ratio is not 1, because the published
  AccessKit root already scales by it.
- **Ctrl and Meta shortcuts on Android** (`flui-platform`): a character key held with Control or
  Meta arrived as an unidentified key, so Ctrl+X, Ctrl+V and every such shortcut missed; its
  character is now looked up without those modifiers, which stay on the event.
- **The Menu key on Android** (`flui-platform`): `KEYCODE_MENU` arrives as `ContextMenu`, the key
  Android's context-menu shortcut is bound to; it arrived unidentified.
- **An input method's Done while it composes** (`flui-platform`): an editor action commits the
  focused field's composition before its Enter, so the field submits the text the keyboard shows
  rather than leaving the Enter to the composition (Samsung Keyboard sends Done without finishing
  its composition).
- **A pen after a hovering mouse on one device** (`flui-platform`): a hovering mouse whose pointer
  comes back as another kind ends its hover and the new contact starts, as a hovering pen's does;
  Android's `input` injector reports mouse and stylus on one device, and the pen's tap was dropped.
