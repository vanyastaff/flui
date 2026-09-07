#!/usr/bin/env bash
# Advisory probe: does wgpu-hal's Metal `acquire_texture` still message AppKit UI
# objects from the calling thread?
#
# ADR-0045 decision 1 pins macOS to the inline raster lane on exactly that fact —
# the occlusion workaround inside `acquire_texture` walks the CALayer tree to the
# backing NSView, sends it `-window`, and sends that NSWindow `-occlusionState`,
# all from whatever thread called `get_current_texture()`. Under a threaded lane
# that is the raster thread, and AppKit UI objects are main-thread-only. The ADR
# records a reopen condition; issue #653 tracks it.
#
# Why this runs after `cargo update --workspace` rather than against Cargo.lock:
# the question worth asking is "has upstream fixed it *yet*", and only the newest
# published version can answer that. A lockfile comparison can report that the
# lock moved, which is not the same question and arrives later.
#
# It is ADVISORY. Every direction this can be wrong is safe: if it says the
# access is still there when upstream has removed it, macOS stays on the inline
# lane, which is slower than necessary and not unsound. Nothing here should ever
# gate a merge.
#
# IT CANNOT ANSWER HALF THE CONDITION. The reopen condition has two disjuncts —
# the access goes away, OR upstream states that doing it off-thread is safe. The
# second is prose; no source predicate sees prose, and it is plausibly the one
# that arrives first (upstream documented precisely this question for
# `display_hdr_info` in 30.0.x). A quiet run means "the code still does it", never
# "the condition is still unmet".
#
# Usage: scripts/probe-metal-acquire-main-thread.sh [path/to/metal/surface.rs]
#
# With no argument it resolves the newest wgpu-hal `cargo metadata` reports. An
# explicit path probes that file instead, which is how the classifier is
# exercised against mutated copies to show it discriminates at all.
set -uo pipefail

say() {
    echo "$*"
    if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
        echo "$*" >> "$GITHUB_STEP_SUMMARY"
    fi
}

if (( $# > 0 )); then
    surface="$1"
    version="source: $surface"
else
    meta=$(cargo metadata --format-version 1 2>/dev/null)
    manifest=$(jq -r '[.packages[] | select(.name=="wgpu-hal")] | sort_by(.version) | last | .manifest_path // empty' <<<"$meta")
    if [[ -z "$manifest" ]]; then
        say "## macOS raster reopen probe: INCONCLUSIVE"
        say ""
        say 'Could not resolve `wgpu-hal` from `cargo metadata`. Nothing was checked.'
        exit 0
    fi
    version=$(jq -r '[.packages[] | select(.name=="wgpu-hal")] | sort_by(.version) | last | .version' <<<"$meta")
    surface="$(dirname "$manifest")/src/metal/surface.rs"
fi

if [[ ! -f "$surface" ]]; then
    say "## macOS raster reopen probe: INCONCLUSIVE (wgpu-hal $version)"
    say ""
    say "\`src/metal/surface.rs\` is gone. The backend was restructured, so the"
    say "citations in \`docs/adr/ADR-0045-raster-lane.md\` need re-reading by hand."
    say "Tracking: https://github.com/vanyastaff/flui/issues/653"
    exit 0
fi

# Scope to the function body. A file-scoped grep is not good enough and would be
# actively misleading here: `display_hdr_info` in the SAME FILE both calls
# `hosting_window` and gates itself on `MainThreadMarker`, so "file contains the
# call and lacks the gate" reads FALSE while `acquire_texture` is still ungated.
body=$(awk '
    /^[[:space:]]*(unsafe )?fn acquire_texture\(/ { inside=1; print; next }
    inside && /^[[:space:]]{4}(pub(\([^)]*\))? )?(unsafe )?fn / { exit }
    inside { print }
' "$surface")

if [[ -z "$body" ]]; then
    say "## macOS raster reopen probe: INCONCLUSIVE (wgpu-hal $version)"
    say ""
    say "Could not locate \`acquire_texture\` in \`$surface\`."
    say "Re-read it by hand: https://github.com/vanyastaff/flui/issues/653"
    exit 0
fi

reaches_appkit=0
grep -qE 'hosting_window|occlusionState|msg_send!\[[^]]*window' <<<"$body" && reaches_appkit=1
gated=0
grep -q 'MainThreadMarker' <<<"$body" && gated=1

if (( reaches_appkit && ! gated )); then
    say "## macOS raster reopen probe: condition NOT met (wgpu-hal $version)"
    say ""
    say "\`acquire_texture\` still messages AppKit UI objects from the calling thread,"
    say "with no main-thread gate. **macOS stays on the inline raster lane** —"
    say "\`docs/adr/ADR-0045-raster-lane.md\` decision 1 is unchanged. No action."
    exit 0
fi

say "## macOS raster reopen probe: **CHANGED** — read this (wgpu-hal $version)"
say ""
if (( ! reaches_appkit )); then
    say "\`acquire_texture\` no longer contains the AppKit access ADR-0045 decision 1"
    say "rests on. The reopen condition's first disjunct may be met."
else
    say "\`acquire_texture\` still reaches an AppKit object but now also mentions"
    say "\`MainThreadMarker\` — it may have been gated the way \`display_hdr_info\` was."
fi
say ""
say "Verify by reading \`$surface\`, then either reopen ADR-0045 decision 1 or record"
say "why it still holds. Do not act on this line alone; it is a grep, not a proof."
say "Tracking: https://github.com/vanyastaff/flui/issues/653"
exit 0
