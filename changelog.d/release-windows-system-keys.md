### Fixed

- Win32 preserves native system keyboard actions such as Alt+F4 when input handlers allow their default behavior, while retaining input prevention and close vetoes.
- Tapping Alt or F10 on Win32 no longer enters menu mode and swallows the next keystroke; Alt+Space still opens the window menu.
