### Fixed

- `Scaffold` places its floating action button at the reading end of the ambient `Directionality`, clear of that side's safe-area inset, and moves it when the direction changes without remounting the page.
- A `Drawer`'s `DrawerAlignment::Start` and `End` resolve against the ambient `Directionality`: under right-to-left the start drawer opens from the right and the end drawer from the left, with the edge strip, its safe-area inset, drag and fling direction and the rounded corners mirrored. A direction change while a drawer is open moves it to the other side at the same progress.
