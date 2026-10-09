### Fixed

- Preserve focus notifications and accepted requests after diagnostic failure; permit router and resampler diagnostic reentry outside mutation guards.
- Distinguish pointer route registrations and preserve current cursor ownership and pending publication across reentry, hook replacement and source removal.
- Refuse overflowing tap-drag displacement and preserve Scale contact generations when user clocks reenter admission.
- Cancel retired scroll contacts on position replacement, publish refresh scroll activity, rebind gesture inertia to the current Vsync owner and preserve repeated Viewer frame samples.
- Retire losing native-scale admissions so subsequent touch gestures remain usable, and preserve valid text clients after failed nested gate installation.
- Publish horizontal accessibility scroll ranges on their actual axis.
- Preserve accepted native replacement Start and cached owner terminal delivery after older cleanup or fresh hit-test failure, retaining the earliest failure and exact admission generation.
- Carry the first contained failure through native staging, claim and cancellation: required cleanup and accepted winner delivery continue while opaque callback captures retain the enclosing failure's ownership policy, and later healthy admission recovers normally.
- Retain opaque native admission retirement ownership under the binding's preserving-close policy instead of invoking further user cleanup after failure.
- Preserve a newer same-actor native Start when older End or Cancelled delivery reenters hit testing or raw observation, including repeated source identities and timestamps.
- Stop Viewer focal inertia when completed layout changes its viewport or boundary, while preserving motion before paint, unchanged rebuilds and zero-elapsed samples; release settled runs without continued Vsync requests.

### Changed

- Construct owner-affine borrowed native gestures with `PanZoomDispatch::new` or `at_root`; struct literals no longer bypass binding admission authority. Dispatch values no longer implement `Send`, `Sync`, `UnwindSafe` or `RefUnwindSafe`; these are intentional pre-1.0 public API breaks.
- `SemanticsNodeData` carries optional typed scroll-axis metadata; exhaustive struct literals must supply it or use a default tail.
