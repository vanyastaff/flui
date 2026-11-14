### Fixed

- The iOS loop delivers a wake deadline that came due (a GPU device rebuild, a refused surface asked for again) as a frame of the scene window and arms the same instant again only after a frame ran, instead of repeating owner turns on the past instant until the display link ticked for another reason.
