### Fixed

- Preserve controller status and run-delivery order during reentrant animation changes, finish healthy status listeners after a panic, and skip listeners removed or disposed during delivery.
- Continue ticking remaining Vsync controllers and child registries after a contained failure; ignore non-finite frame instants before changing run anchors.
