### Changed

- `Navigator` starts a route's entrance from the previous push's completion instead of from
  `build`, in the same frame as before; `ScaffoldMessenger` settles its snack bars and banners
  from their animation status and from `show`/`hide` calls instead of from `build`. An
  `on_closed` that an animation tick settles runs in the frame's post-frame phase.
