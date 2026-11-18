# ADR-0382: A presentation drives its animation registry through a `VsyncDriver`

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Related:** [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) §4 (composition-root
  seams), [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md) (owner-local registries
  and their close), [ADR-0191](ADR-0191-single-parent-animation-registries.md) (one parent per
  registry)

## Context and problem statement

`flui_animation::Vsync` was two things at once. It is the handle a tree hands its controllers
through `VsyncScope`: a widget registers a controller on it, nests a `TickerMode` registry under it
with `attach_child` and mutes it. It was also the frame driver: `tick_all` advanced every
controller below it, `set_frame_requester` and `set_build_phase` bound it to a presentation, and
`prepare_close` closed it for good. Only `flui-runtime` calls the second group, but every handle
carried it, and `flui::animation` showed it to every application. A widget that read the ambient
registry could tick its whole presentation's animations out of frame, swap the presentation's frame
requester or close the registry under the runtime.

## Decision drivers

- The wrong call does not compile: a handle cannot advance, rebind or close a registry.
- The runtime keeps one owner per presentation registry, with no extra indirection on the frame
  path.
- Tests that drive a registry without a host keep doing so, in the same form the runtime uses.
- The seam follows the existing composition-root pattern (ADR-0081 §4): hidden, shadowed in the SDK
  and the facade.

## Considered options

1. **Keep the methods on `Vsync`** and document that only a frame driver calls them.
2. **A sealed extension trait** in `flui_animation::__runtime`, implemented for `Vsync`, carrying the
   driver methods (the `BindingRuntime` pattern of `flui-view`).
3. **A driver type** in `flui_animation::__runtime` that owns the root registry and hands out its
   handle; the driver methods live only on it.

## Decision

Chosen option: 3, because the driver's authority becomes a value the presentation owns, so no
handle reached from a tree can be turned into one. Option 1 leaves the defect. Option 2 hides the
methods from the default import, but any crate that imports the trait can still tick any handle it
holds, including one read from `VsyncScope`.

```rust
// flui_animation::__runtime (#[doc(hidden)])
pub struct VsyncDriver { /* the root Vsync */ }

impl VsyncDriver {
    pub fn new() -> Self;
    pub fn vsync(&self) -> &Vsync;
    pub fn set_frame_requester(&self, request_frame: Option<Rc<dyn Fn()>>);
    pub fn set_build_phase(&self, phase: Option<BuildPhase>);
    pub fn tick_all(&self, tick: &FrameTick);
    pub fn tick_all_with_recovery(&self, tick: &FrameTick, recovery: &mut RecoveryScope<'_>);
    pub fn prepare_close(&self) -> VsyncRetirement;
}
pub use crate::vsync::VsyncRetirement;

// flui_animation
impl Vsync {
    pub fn new() -> Self;
    pub fn attach_child(&self, child: &Vsync) -> Result<VsyncRegistration, VsyncRegistrationError>;
    pub fn detach_child(&self, id: &VsyncRegistration);
    pub fn set_muted(&self, muted: bool);
    // is_muted, is_building, has_running, len, is_empty, is_same
}
```

1. **One driver per root.** Each presentation owns a `VsyncDriver` and gives its tree
   `driver.vsync().clone()`. The driver is not `Clone`, and nothing constructs one from a handle.
2. **Handles register, nest and mute.** A registry a widget creates with `Vsync::new` advances only
   while it is attached under a driven registry. Read-only queries (`has_running`, `len`,
   `is_building`) stay on the handle.
3. **Ticking an attached registry.** A driver's registry may be attached under another; ticking it
   directly keeps the clock authority of its live parent chain, as before.
4. **The seam is not surface.** `flui_animation::__runtime` is `#[doc(hidden)]`. `flui_sdk::animation`
   and `flui::animation` are glob modules that shadow it, so neither path resolves. Its users are
   `flui-runtime` and tests that drive a registry without a host; a package's tests depend on
   `flui-animation` directly for it.

### Confirmation

The doctest on `VsyncDriver` fails to compile when a `Vsync` handle calls `tick_all`
(`compile_fail,E0624`). The `compile_fail,E0603` doctests on `flui_sdk::animation` and
`flui::animation` show the seam does not resolve through either. `flui-runtime`'s presentation
holds a `VsyncDriver` and ticks through `vsync_driver()`; its frame-path and close suites
(`frame_pipeline_and_vsync`, `tests/contracts/owner_close.rs`) keep their assertions.

## Pros and cons of the options

### 1. Methods on `Vsync`

- Good, because nothing changes.
- Bad, because any widget can drive or close its presentation's registry.

### 2. A sealed extension trait

- Good, because call sites change only by an import.
- Bad, because the authority still travels with every handle: an import grants it.

### 3. A driver type

- Good, because the authority is owned, and a handle cannot reach it.
- Good, because the runtime's field says what it is: the driver of the presentation's registry.
- Bad, because every test that ticks a registry by hand declares a driver and takes its handle.
