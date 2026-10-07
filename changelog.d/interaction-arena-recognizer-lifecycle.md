### Added
- `GestureRecognizer::add_pointer_down` admits a pointer from its `Down` dispatch, so a recognizer reads the device kind and pressed button; `GestureDetector` and the back gesture now admit recognizers through it.

### Fixed
- A tap held by a double tap's window is no longer lost when the next click lands elsewhere or after the window, including with a mouse's single pointer ID; a far second contact delivers the first tap at once and starts a new double-tap window.
- A second finger no longer resets a running drag, long press or double tap without an end or cancel callback, and another finger's events no longer end them.
- Tap, double tap and long press cancel on the device kind's own slop (a mouse no longer drifts 18 px); drag and long press ignore non-primary buttons; drag reports the device kind of its `Down`.
- Drag and multi-drag velocity samples use the events' own timestamps instead of dispatch time.
- A callback may dispose its own tap, double tap or long press recognizer; a panicking double-tap or multi-tap callback no longer wedges the recognizer or leaves an arena held; a panicking team member no longer skips its teammates' rejections.
- The gesture arena ignores an accept from a member that already withdrew, and a held arena swept on pointer up no longer blocks the pointer's next contact.
