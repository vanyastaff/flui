# AGENTS.md — flui-animation

Animation system: controllers, curves, tweens, and spring values. Persistent objects that survive widget rebuilds.

## What lives here

- **`Animation<T>` trait** — base trait for all animations (extends `Listenable`, object-safe, Send+Sync+Debug)
- **`AnimationController`** — primary driver (generates 0.0..1.0), requires `Scheduler`
- **`CurvedAnimation`** — applies easing curves to animations
- **`Curve` trait + `Curves`** — easing curves (full Penner catalog, M3 `ThreePointCubic` emphasized set, `Split`)
- **`Tween<T>`** — maps animation values; `OklabColorTween` for perceptual color interpolation
- **`smoothing` module** — frame-rate-independent followers: `exp_decay`/`Smoothed` (half-life), `SmoothDamp` (critically damped)
- **`AnimatedValue`** — interruptible spring value with velocity-preserving retargeting
- **`#[derive(Animatable)]`** — via `flui-macros`, for custom spring-animatable types

## Key constraints

- **Persistent object pattern** — animation objects are `Arc`-based, survive widget rebuilds. Create once outside `build()`, use many times inside.
- **`serde` feature** — optional serialization for animation types. Forwards to `flui-types/serde`.
- **Benchmark** — `animation_bench` (criterion).
- **No `DynAnimation`** — deleted in redesign. `Animation<T>` is the only object-safe trait.
- **Oklab color interpolation** — `OklabColorTween` interpolates in perceptual color space instead of componentwise sRGB.

## Related crates

- `flui-scheduler` — provides `Scheduler` for frame timing and ticker coordination
- `flui-macros` — provides `#[derive(Animatable)]`
- `flui-foundation` — provides `Listenable` trait and `ListenerRegistry`
