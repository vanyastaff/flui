### Fixed

- Win32: a mouse press now captures the mouse until its last button is released, so a drag released outside the window delivers its release instead of leaving the drag stuck; a capture taken away mid-press (another window, a modal loop, `WM_CANCELMODE`) ends the sequence with a pointer cancel.
- Win32: pointer and keyboard events report the modifiers held when their message was generated (`GetKeyState`), not the keyboard state at processing time.
