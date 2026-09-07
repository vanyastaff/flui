#!/usr/bin/env bash
# Advisory probe: are the two upstream facts that FAIL SILENTLY still true?
#
# Issue #983 sorts the upstream facts this workspace depends on by what happens
# when they move. Three of the five already fail LOUDLY and need nothing here:
#
#   - `Window::pre_present_notify` arming Wayland's frame callback -- live-smoke's
#     `check_frame_signal_armed_before_every_present` bails when the per-present
#     notify count stops matching the present count.
#   - which winit backends emit `Occluded` -- live-smoke's occlusion check times
#     out after 10s with no skip path if the signal never arrives.
#   - winit's `Ime` enum shape -- `platforms/winit/events.rs`'s `ime_event` matches
#     it exhaustively with NO wildcard arm, and winit's `Ime` is not
#     `#[non_exhaustive]`, so any variant change breaks the build.
#
# The two below have citations and nothing else. Both are depended on by shipped
# behaviour, and both would move without a panic, a compile error, or a red test.
#
#   1. winit calls `XInitThreads()` before `XOpenDisplay`. ADR-0045 decision 1
#      leans on this to permit off-thread X11 surface work AT ALL. If winit stops,
#      that becomes undefined behaviour and Xlib will not say so.
#   2. wgpu-hal configures the Vulkan swapchain with
#      `min_image_count(maximum_frame_latency + 1)`. ADR-0058's deadline pacer was
#      measured against the two images that yields at our latency of 1; a changed
#      formula makes the arithmetic wrong on every platform, and the symptom is a
#      frame-rate regression nobody attributes to a dependency bump.
#
# ADVISORY, never a gate. A false "still true" costs nothing; a false "changed"
# costs one read. The forcing function is the winit 0.31 bump (ROADMAP-TRACKER
# H10), which should not land without these re-verified.
#
# Predicates are by CONTENT, never by line number -- citation rot into a moving
# dependency is the defect this whole issue family exists to stop.
set -uo pipefail

say() {
    echo "$*"
    [[ -n "${GITHUB_STEP_SUMMARY:-}" ]] && echo "$*" >> "$GITHUB_STEP_SUMMARY"
    return 0
}

say "## Upstream facts that fail silently (issue #983)"
say ""

meta=$(cargo metadata --format-version 1 2>/dev/null)
if [[ -z "$meta" ]]; then
    say "- **INCONCLUSIVE.** \`cargo metadata\` produced nothing; nothing probed."
    exit 0
fi

resolve() { jq -r --arg n "$1" '.packages[] | select(.name==$n) | "\(.version)\t\(.manifest_path)"' <<<"$meta" | head -1; }

# --- 1. XInitThreads before XOpenDisplay -------------------------------------
entry=$(resolve winit)
if [[ -z "$entry" ]]; then
    say "- **winit — INCONCLUSIVE.** not in this resolution."
else
    ver="${entry%%$'\t'*}"; dir=$(dirname "${entry#*$'\t'}")
    f="$dir/src/platform_impl/linux/x11/xdisplay.rs"
    if [[ ! -f "$f" ]]; then
        say "- **winit $ver — INCONCLUSIVE.** \`x11/xdisplay.rs\` is gone; the X11 backend"
        say "  was restructured, so ADR-0045's X11 bullet needs a hand re-read."
    else
        init=$(grep -n 'XInitThreads' "$f" | head -1 | cut -d: -f1)
        open=$(grep -n 'XOpenDisplay' "$f" | head -1 | cut -d: -f1)
        if [[ -z "$init" ]]; then
            say "- **winit $ver — CHANGED.** \`XInitThreads\` is no longer called in"
            say "  \`xdisplay.rs\`. ADR-0045 decision 1 permits off-thread X11 surface work"
            say "  ONLY because it was. Re-read before the next raster-lane change."
        elif [[ -n "$open" && "$init" -gt "$open" ]]; then
            say "- **winit $ver — CHANGED.** \`XInitThreads\` is called AFTER \`XOpenDisplay\`."
            say "  Xlib requires it first; the ordering ADR-0045 relies on no longer holds."
        else
            say "- **winit $ver — unchanged.** \`XInitThreads\` still precedes \`XOpenDisplay\`."
        fi
    fi
fi

# --- 2. min_image_count(maximum_frame_latency + 1) ---------------------------
entry=$(resolve wgpu-hal)
if [[ -z "$entry" ]]; then
    say "- **wgpu-hal — INCONCLUSIVE.** not in this resolution."
else
    ver="${entry%%$'\t'*}"; dir=$(dirname "${entry#*$'\t'}")
    f=$(find "$dir/src/vulkan" -name '*.rs' -exec grep -ln 'min_image_count' {} + 2>/dev/null | head -1)
    if [[ -z "$f" ]]; then
        say "- **wgpu-hal $ver — INCONCLUSIVE.** no \`min_image_count\` under \`src/vulkan\`;"
        say "  the swapchain path moved. ADR-0058's pacing premise needs a hand re-read."
    elif grep -q 'min_image_count(config.maximum_frame_latency + 1)' "$f"; then
        say "- **wgpu-hal $ver — unchanged.** swapchain still requests"
        say "  \`maximum_frame_latency + 1\` images; ADR-0058's pacer arithmetic holds."
    else
        say "- **wgpu-hal $ver — CHANGED.** the swapchain no longer requests exactly"
        say "  \`maximum_frame_latency + 1\`. ADR-0058's deadline pacer was measured against"
        say "  that formula on EVERY platform. Re-measure before trusting the pacing."
        grep -n 'min_image_count' "$f" | head -3 | while read -r l; do say "  - \`$l\`"; done
    fi
fi

say ""
say "Advisory — never a gate. Neither fact can be seen by a test, a panic, or the"
say "compiler; that is why they are probed. Tracking: https://github.com/vanyastaff/flui/issues/983"
