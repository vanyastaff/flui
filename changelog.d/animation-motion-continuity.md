### Added

- Derive `TwoWayConverter` for nested fixed-width values, including geometry
  and colors, while preserving each field's interpolation contract.

- Browser hosts observe reduced motion before canvas creation and deliver live
  changes through owner turns; wake and quit proxies use the same host signal.

- Native macOS and iOS hosts observe reduced motion; Android preference sampling
  observes and validates the system animation-duration scale through the
  existing host preference source.
- The macOS winit fallback shares the AppKit preference sampler and retires it
  with its event-loop owner, including during unwinding.
- Linux hosts observe reduced motion through the Settings portal, with the
  GNOME enable-animations fallback and legacy portal support. Failed reads
  preserve the accepted motion preference and retry without another notification.

- Add the interactive `motion_lab` example with Full/Reduce/FollowSystem modes,
  property interruption, independent deadlines, swipe and drawer transitions.
- Headless hosts and laid-out widget harnesses can set the application motion
  preference through the runtime's existing override path.

- Resolve host motion preferences through presentation clocks, with application
  FollowSystem/Reduce/Full overrides and explicit Normal/Preserve controller
  behavior. Publish resolved policy through `MediaQuery::motion_of`.

- `AnimationController::fling_across` converts physical gesture velocity across
  a validated extent and the authored controller range before admitting motion.

- Scalar controller retargeting between curve and spring motion preserves the
  last published position and velocity and cancels the displaced run.
- `AnimatedValue` owns one Vsync registration for all components, exposes a
  surviving observer stream, and supports atomic target and motion replacement.
- `MotionUpdate` coordinates independent property owners, retaining prepared
  segments and publishing their deliveries only after every owner is admitted.
- Animation status subscriptions own removal authority through
  `StatusSubscription`; dropping a guard removes its callback without retaining
  the animation owner, while detaching leaves it registered until source closure.
- Opacity, padding, alignment, numeric container properties and rotation support spring motion and retain their incoming
  velocities when retargeted through the render, layout and transform paths.

### Changed

- `TwoWayConverter` derives require at least one motion field; empty unit,
  named and tuple structs are rejected rather than producing empty vectors.

- `MediaQueryData` gains a `motion` field; exhaustive struct literals must supply
  it or use `..Default::default()`. Essential timers, physical inertia and loading
  indicators retain their timing under reduced motion and host duration scales.

- Replace manual `AnimatedValue::advance` and owner cloning with frame-driven
  ownership. Component vectors are fixed arrays; observer views remain cloneable.
- Custom `Animation<T>` implementations provide `subscribe_status`, returning
  source-bound removal authority; framework relays share delivery recovery.
- Scroll and page animation methods accept `ArcCurve`. Replacing programmatic
  scroll motion retains its published velocity, including when braking at the
  current position.

### Removed

- Unused `AnimationBehavior::should_preserve` and `is_normal` predicates;
  select behavior on the controller builder and match the enum when needed.

- Shared normalized implicit controllers and optional generic property tweens;
  matrix interpolation retains its concrete decomposition tween.
- Manual animation status listener IDs and source-selected status removal.
  Retain the returned subscription to control its lifetime, or detach it to
  leave the callback registered until source closure.

### Fixed

- Cancel pending Hero creation and queued measurements when their controller is
  replaced. Builder failure or cancellation during creation and diversion restores
  the selected heroes; remaining matched launches preserve the first failure.
- Refuse stale Hero retirement when another flight has reused its tag.

- Reversing an airborne Hero flight retraces its custom rect mapping without
  switching paths; redirection to a different hero selects the new path.

- Skip redundant retirement recovery when an already disposed animation owner
  is disposed again or subsequently dropped.

- Release the Hero rect-factory guard before calling its factory, mapping or
  destructor, allowing each to reenter the same flight without deadlocking.
- Preserve a Hero mapping's evaluation failure through opaque mapping retirement;
  a competing destructor failure no longer aborts before frame containment.
- Give `Animatable` an associated `Value` type. Manual implementations now
  declare `type Value`; `TweenAnimation` and `ReverseTween` take only the mapping
  type. Hero rect mappings accept owner-local captures.

- Scalar and geometry interpolation preserve representable extrapolations
  when the intermediate multiplication overflows before adding the start.

- Preserve representable scalar and composite geometry interpolation between
  opposite finite extremes, and retain authored endpoint bits without clamping
  extrapolated motion.

- Keep accessibility bounds and clips aligned with animated and nested paint
  transforms during full assembly and partial updates; refuse unrepresentable
  projected bounds and restore them when finite geometry returns.

- Retiring the final platform background-executor owner from an async task no
  longer panics during runtime shutdown, including when a stopped host's last
  proxy is released after Linux preference observation.

- Rebuild only the RefreshIndicator overlay when refresh starts or finishes;
  retain scroll content and gesture handlers across phase changes.

- Move Dismissible content through a render-owned slide while delivering
  progress updates independently of element rebuilds.
- Move Drawer panels and fade their scrims through render-owned transitions,
  removing element rebuilds on settling animation ticks.
- Preserve the dragged position when Dismissible releases in the opposite direction.
- Use Dismissible's actual laid-out size for drag, fling and collapse under loose
  or unbounded incoming constraints, and remove its constraints-only LayoutBuilder.
- Retain Drawer gesture ownership while opening changes its painted content;
  gesture release preserves physical speed across panel widths.
- Preserve representable motion rates when intermediate multiplication underflows.
- Refuse invalid Container or Align motion before changing any property run;
  preserve prior matrix progress when replacement motion cannot be prepared.
- Release rejected optional-owner registrations after preparation panic and
  close every removed owner before grouped cancellation delivery.
- Register Container transform progress only while its matrix is present;
  withdraw disappearing motion and coordinate Align/Container teardown.
- Retain container size and color motion through interruption; independent
  properties keep their own deadlines, and non-finite targets preserve live motion.
- Keep AnimatedAlign factors on their own motion deadlines when alignment changes;
  preserve optional factor constraints-fill behavior and refuse non-finite targets.
- Return inert status subscriptions after controller or switch disposal and
  release state borrows before refusing exhausted controller identities.
- Finish interruptible spring motion continuously at its exact target, preserving
  velocity through the rest transition and removing dependence on the last frame.

- Keep controller sample time and pending playback changes intact when a curve
  panics, returns a non-finite position, or a simulation completion query fails.

- Report a curved controller's instantaneous velocity at its last sampled time, including direction and playback rate, instead of its average run velocity.
- Refuse stale derivatives after a curve or simulation reenters the controller; preserve the first failure through derivative-source retirement and keep representable scaled velocities finite.
- Request pending motion settlement and parked resumption when a registry is
  unmuted, its frame requester is replaced or its child is attached, including
  paused playback and policy changes deferred by an ancestor gate.
- Request a fresh policy sample when a parked repeat is rebound to another
  presentation, including a registry that has not received its first tick.
- Resume Preserve repeats on a fresh registry under Reduce after their previous
  clock is absent or exhausted; resumption honors the selected timeline's capacity.
- Keep single-drawer drag geometry in the render path when crossing halfway;
  update scaffold structure only when its two drawer slots must change order.
- Preserve accepted Dismissible resize notifications before completion, including
  when several animation samples arrive before a build or a resize callback panics.
- Keep floating-header snap commands in owner-local state and refuse exhausted
  command epochs without wrapping or replacing the last admitted command.
- Continue active Scrollable fling and wheel trajectories when their Vsync
  registry changes, retaining sampled position and accumulated wheel goals.
- Commit all Switcher and Dismissible controller bindings before migration wakes;
  restore shared collapse ownership before callback reentry. `VsyncUpdate::prepare`
  returns a publication whose explicit delivery or drop completes the accepted tail.
- Use component-specific spring rest distances through
  `TwoWayConverter::rest_thresholds`; derived values compose their fields'
  distances. Manual converters must implement the new method with positive finite
  distances in their vector units. Invalid distances preserve the previous run.
- Cancel every presentation's animation before runtime shutdown callbacks,
  including when an earlier cancellation fails and widget owners are retained.
  Closed Vsync registries refuse registration with `VsyncRegistrationError::Closed`;
  saved observers cannot start a new run or revive a closed kernel by rebinding.
- Preserve AnimatedSize completion order relative to later owner post-frame
  events. Completion directly enters the existing owner lane without a counter
  or completion-driven rebuild; current callbacks, unmount cancellation and
  accepted delivery after callback failure follow the lane's contract.
- Withdraw stale animation frame demand when motion is paused or frame delivery
  is disabled. Explicit inspection steps retain their own host demand, including
  after run completion, while independent widget builds remain deliverable.
