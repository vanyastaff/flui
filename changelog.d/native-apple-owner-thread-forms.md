### Fixed

- macOS: a press on the title bar an application draws now moves a window that was not key on the click that activates it, as a system title bar does; a click on the content still only activates the window.
- macOS: a closed window no longer leaks its window delegate or its mouse tracking area, and a window whose construction stops early releases its `NSWindow`.
