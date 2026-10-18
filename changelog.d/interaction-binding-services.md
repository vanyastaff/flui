### Added

- `flui_interaction::InputSurface` names the routing root of an input stream: a presentation's window, or a surface embedded in it that `GestureBinding::embed_surface` mints. `GestureBinding::dispatch_pointer_on` routes input a surface reported; hover, move coalescing and wheel and pan-zoom latches are kept per surface, and a contact stays with the surface that admitted its Down when another surface reports its tail.

### Changed

- A `GestureBinding` closed from one of its own callbacks withdraws its arena and pointer router at once and retires the rest of its state (arena rejections, route release, capture loss) when the outermost dispatch returns, instead of inside the callback that closed it.
