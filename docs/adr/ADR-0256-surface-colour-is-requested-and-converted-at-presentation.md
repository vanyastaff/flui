# ADR-0256: A window asks for a surface colour capability, the renderer paints in one working space and converts at presentation

- **Status:** Accepted (2026-10-26). Not implemented on Apple: the Metal colour spaces are in the
  candidate list but have no run behind them (Confirmation).
- **Date:** 2026-10-26
- **Related:** [ADR-0254](ADR-0254-window-contract.md) (the per-window configuration the request
  joins), [ADR-0244](ADR-0244-os-version-floors-and-gates.md) (capabilities are asked for, never
  derived from a version)
- **Sources:** wgpu 30.0.1 `wgpu/src/lib.rs` ("Surface color spaces and HDR output": what each
  `SurfaceColorSpace` expects the shader to write) and `wgpu-types` `DisplayHdrInfo::tone_map_headroom`;
  ITU-R BT.2408 (203 nits reference white for HDR10 graphics); ITU-R BT.2100 (PQ).

## Context and problem statement

The renderer configures every window surface as 8-bit sRGB (`Bgra8Unorm` or `Rgba8Unorm` with
`SurfaceColorSpace::Srgb`). On a Display P3 or HDR display the compositor therefore receives sRGB:
the app is correct but never wider or brighter than SDR, and no value above white can be drawn.
The shaders write sRGB-encoded values and blend in that encoded space; gradients, the text gamma
tables and the blend oracles all assume it. wgpu 30 can configure wide-gamut and extended-range
colour spaces and reports the display's live headroom, but each of them expects the shader to write
that space's own gamut and transfer function.

Which part of the surface configuration belongs to the application, what the renderer paints in,
where the conversion to the surface happens, and what happens to a highlight the display cannot
show?

## Decision drivers

- A request the surface cannot meet must degrade to something that works and say so; a missing
  capability is visible, not silent.
- Breaking API is acceptable pre-release; the request must leave room to grow (new spaces, per-window
  differences) without another break.
- Pixels of every existing app must stay identical unless the app asks for more.
- The frame path is synchronous; no extra pass for the default configuration.
- CI executes Linux only: the conversion must be provable on a software adapter.

## Considered options

1. Select the widest pair the surface offers, always.
2. A `#[non_exhaustive]` request type per window, the renderer resolving it against the surface.
3. Move the whole renderer to linear light.

## Decision

Chosen option: 2, with the working space of option 1's wide path and without option 3.

### §1. The request is `SurfaceColor`, per window

`flui_engine::SurfaceColor` is `#[non_exhaustive]` with `Srgb8` (the default), `WideGamut` (an SDR
surface wider than sRGB) and `Hdr` (an extended-range surface). `AppConfig::surface_color` carries
it for the window the config opens; `Renderer::with_surface_color` takes it and `Renderer::new` is
`Srgb8`. It is a request: the renderer falls back `Hdr` → `WideGamut` → `Srgb8`, and
`Renderer::surface_color()` returns a `ResolvedSurfaceColor` with the granted `SurfaceColor`, the
swapchain format and colour space, the working format and the headroom. A downgrade is logged once
at `warn` on target `flui.gpu` (`event = "surface_color_downgraded"`) and `is_downgraded()` answers it.

### §2. Selection reads the surface's capabilities and the display's state

`ResolvedSurfaceColor::resolve(requested, caps, hdr, working_renderable)` is the one place the pair
is chosen. For `Hdr`, in order: `Rgba16Float` with `ExtendedDisplayP3`, `ExtendedSrgbLinear`,
`ExtendedSrgb`, then `Rgb10a2Unorm` with `Bt2100Pq`. `WideGamut` takes an 8-bit format with
`DisplayP3`. `Srgb8` is the existing pair. Three conditions skip the wider pairs and fall back:
the adapter cannot render to and blend `Rgba16Float`; the display reports no headroom over SDR white
(`DisplayHdrInfo::tone_map_headroom() <= 1.0`, which wgpu derives from a definitively-SDR display);
the surface lists no such pair. A surface with no 8-bit sRGB pair is refused with
`EngineError::UnsupportedSurfaceColorConfiguration` whatever was requested, as before. A colour
space is never chosen by `Auto`: an FP16 surface would otherwise brighten encoded values.

### §3. One working space: extended-range sRGB-encoded, premultiplied, blended encoded

Painting does not change with the request. Pixels are sRGB-encoded, premultiplied, and blended in
that encoded space. `Srgb8` paints straight into the swapchain as before. Any other granted
configuration paints into a retained `Rgba16Float` target, whose range holds values above `1.0`
and below `0.0` (out-of-sRGB-gamut colours), and the frame always takes the retained-target path.
Linear-light blending is not adopted: it changes the look of every translucent edge, gradient and
glyph in every app and invalidates the oracles that pin them, so it is a separate decision with its
own migration if it is wanted.

### §4. The presentation blit converts to the surface

The blit that writes the retained target to the swapchain (`encode_presentation`) takes an
`OutputEncoding`: `Identity` (copy, as before) or a transform: un-premultiply, decode sRGB
(sign-preserving), tone-map (§5), multiply by the primaries matrix, encode, re-premultiply.

| Colour space | Primaries | Written value |
|---|---|---|
| `DisplayP3` | P3 | sRGB-encoded, clamped |
| `ExtendedDisplayP3` | P3 | sRGB-encoded, range kept |
| `ExtendedSrgb` | BT.709 | sRGB-encoded, range kept |
| `ExtendedSrgbLinear` | BT.709 | linear, `1.0` = SDR white |
| `Bt2100Pq` | BT.2020 | PQ of `linear × 203 / 10000` |

Blit pipelines are cached per swapchain format. A recreated surface that moves only the swapchain
format reuses the painter; one that moves the working format rebuilds it, as for any format change.

### §5. Tone mapping is hue-preserving and bounded by the headroom

For the brightest channel `p > 1` and `h` the display headroom (a multiple of SDR white), with
`x = (p - 1) / (h - 1)`, the channel maps to `1 + (h - 1) · x / (1 + x)` and the other two scale by
the same factor. The curve is the identity up to `1.0`, continuous in slope there, and bounded by `h`.
SDR outputs use `h = 1.0`, which clips the peak to white while keeping hue. An HDR display whose
peak is not reported uses `h = 4.0`. The headroom is read when the surface is configured.

### §6. Failure paths

- A request not met is a downgrade, not an error (§1); only a surface without the 8-bit sRGB pair
  fails.
- Device recovery and surface recreation resolve the original request again against the new
  capabilities; `surface_color()` follows the surface.
- The display entering or leaving HDR mode while the app runs is not observed: the configuration
  changes at the next recreation. The change-event row in `docs/platform-parity.md` stays ❌.

## Pros and cons of the options

### 1. Widest pair, always

- Good, because apps get HDR without asking.
- Bad, because values meant as SDR white appear at whatever the display maps `1.0` to, and
  tone-mapping policy is hidden from the app.

### 2. Request type, resolved by the renderer

- Good, because the default is unchanged and the result is inspectable.
- Good, because a new variant or space adds no break.
- Bad, because an app must opt in, and the working target costs memory and a blit.

### 3. Linear light for everything

- Good, because blending is physically correct.
- Bad, because every pixel of every app changes; a larger decision than this one.

## Consequences

- Good: P3 and extended-range displays can be driven; nothing changes for an app that does not ask.
- Good: the conversion is a pure function with a CPU reference, tested on a software adapter.
- Bad: a non-`Srgb8` frame costs an `Rgba16Float` target and a full-screen pass.
- Bad: wide-gamut and HDR sources (P3 images, video, HDR images) still enter as sRGB values; the
  working format has room for them, the sources are future work.
- Neutral: `docs/gaps.md` entry 24 narrows to the platform-layer headroom read, change events,
  Apple hardware runs and wide-gamut sources.

### Confirmation

- `the_presentation_blit_matches_the_reference_conversion` (every transfer and gamut against
  `convert_pixel`, an over-white highlight, a translucent pixel).
- `a_wide_gamut_frame_reaches_the_surface_through_the_working_target` (the painter in
  `Rgba16Float`, presented to an 8-bit P3 surface).
- `hdr_takes_the_first_extended_pair_in_preference_order`,
  `hdr_on_a_display_without_headroom_falls_to_wide_gamut_then_srgb`,
  `an_adapter_that_cannot_blend_the_working_format_gets_srgb8`,
  `a_surface_without_the_sdr_pair_is_refused_whatever_was_asked` (selection).
- `tone_mapping_is_the_identity_up_to_white_and_bounded_by_the_headroom`.
- Not run: any HDR or P3 display on Windows, Linux or the Web, and every Metal space. Those cells in
  `docs/platform-parity.md` stay ❌ until a run on that platform.

## More information

Revisit when a wide-gamut source type lands (images with a profile, video), when a host reports
HDR changes, or when linear-light blending is proposed.
