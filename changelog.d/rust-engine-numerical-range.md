### Fixed
- GPU curve and circle scale estimates and dashed segment lengths avoid intermediate squared-length overflow. Non-finite dashed segments reject the whole stroke and allow the next draw to proceed.
- Dashed strokes refuse an interval that cannot advance at raster precision, discard the whole primitive and allow the next draw to proceed.
- Dashed closed contours preserve the configured join at covered seams, including a dash wrapping across the starting point; gaps retain their caps.
