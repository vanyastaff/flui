### Changed

- The boundary every OS entry goes through is `flui_foundation::panic::os_entry`, with `AfterPanic`, `ContainedIvars` and `contain_os_entry`, below both `flui-native` and the capability crates. `flui-system` had a copy of its own that had drifted: an answer after a panic that panicked too unwound out of the entry instead of ending the process, a class's ivars whose destructor panicked were logged without their type, and an entry with no arguments logged `()`. It now calls the one boundary.
