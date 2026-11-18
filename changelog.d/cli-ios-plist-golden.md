### Added
- `flui build ios` writes `UISupportedInterfaceOrientations~ipad` with all four orientations, which iPad multitasking requires unless an application sets `UIRequiresFullScreen`; a golden test pins the whole generated `Info.plist`.
