### Changed

- `InkWell` makes its state layer's animation at the first state that shows the layer and lets it go once the well is back at rest, so a resting well holds no animation controller and no frame registration (an `InkWell` over a `Text` mounts with 69 allocations and 8.7 KB, from 83 and 11.3 KB).
