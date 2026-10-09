### Fixed

- Preserve focus notifications and accepted requests after diagnostic failure; permit router and resampler diagnostic reentry outside mutation guards.
- Distinguish pointer route registrations and preserve current cursor ownership and pending publication across reentry, hook replacement and source removal.
- Refuse overflowing tap-drag displacement and preserve Scale contact generations when user clocks reenter admission.
- Cancel retired scroll contacts on position replacement, publish refresh scroll activity, rebind gesture inertia to the current Vsync owner and preserve repeated Viewer frame samples.
- Retire losing native-scale admissions so subsequent touch gestures remain usable, and preserve valid text clients after failed nested gate installation.
- Publish horizontal accessibility scroll ranges on their actual axis.

### Changed

- Construct borrowed native gestures with `PanZoomDispatch::new` or `at_root`; struct literals no longer bypass binding admission authority.
- `SemanticsNodeData` carries optional typed scroll-axis metadata; exhaustive struct literals must supply it or use a default tail.
