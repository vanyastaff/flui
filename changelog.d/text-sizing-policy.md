### Fixed

- Keep a scene's measured surface generation through raster submission, refusing
  stale scenes after resize or surface replacement until layout samples the new
  surface.
- Release the native raster-lane guard around runtime frame callbacks, retaining
  repaint and recovery demand when backend access is temporarily busy.
- Publish authored icon accessibility labels and keep decorative font codepoints
  out of spoken labels.

### Changed

- Replace `MediaQueryData::text_scale_factor` with explicit `text_sizing` policy.
  Fixed, linear and exact size/profile answers share measurement across ordinary
  text, editable text and icons; nearest providers replace outer policy.
- Preserve numeric growth profiles through authored text-style inheritance and
  Material/Cupertino typography defaults.
- Return `TextMeasurementError` from painter measurement, distinguishing missing
  sizing answers from invalid authored input. Low-level shaping continues to
  return `TextLayoutError`.
- Keep numeric sizing policies, answer leases, preparation debt and painters
  owner-local. Shaped output and display lists remain transferable to raster
  work; text sizing adds no shared lock to the frame path.
- Resume native text preparation with the presentation's original animation
  sample, retaining completion callbacks until geometry is ready. Native sizing
  service releases runtime loans, rejects obsolete replies and preserves bounded
  recovery; unavailable geometry does not indefinitely block document repair.
