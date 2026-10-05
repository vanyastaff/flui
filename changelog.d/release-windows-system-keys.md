### Fixed

- Win32 preserves native system keyboard actions such as Alt+F4 when input handlers allow their default behavior, while retaining input prevention and close vetoes. FLUI apps now allow the default for any key no shortcut or focused widget consumed, so Alt+F4 and Alt+Space work in real apps.
- A Win32 timer or gesture deadline that falls due while every window is minimized now runs instead of waiting for unrelated input.
- Tapping Alt or F10 on Win32 no longer enters menu mode and swallows the next keystroke; Alt+Space still opens the window menu.
