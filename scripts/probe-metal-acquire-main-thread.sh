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
# the question worth asking is "has upstream fixed it *yet*", and a comparison
# against the committed lock can only report that the lock moved, which is a
# different question arriving later.
#
# HOW FAR `cargo update` ACTUALLY REACHES, stated because it is easy to overclaim:
# the workspace pins `wgpu = "30.0"`, and `cargo update` does not cross a semver
# major boundary (that is `--breaking`, still unstable). So this reaches the
# newest COMPATIBLE wgpu-hal, not the newest published one. If upstream fixes
# `acquire_texture` in wgpu 31, no amount of `cargo update` will show it here.
# The script therefore also asks crates.io what the newest release is and says
# so loudly when that is out of the probe's reach -- which is the case the
# reopen condition is most likely to arrive in.
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

# Classify one `metal/surface.rs`. Prints its verdict; sets `verdict` to
# not-met / changed / inconclusive for the caller's summary line.
classify() {
    local surface="$1" label="$2"

    if [[ ! -f "$surface" ]]; then
        verdict=inconclusive
        say "- **$label — INCONCLUSIVE.** \`src/metal/surface.rs\` is gone; the backend was"
        say "  restructured, so ADR-0045's citations need re-reading by hand."
        return
    fi

    # Scope to the function body. A file-scoped grep is not good enough here and
    # would be actively misleading: `display_hdr_info` in the SAME FILE both
    # calls `hosting_window` and gates itself on `MainThreadMarker`, so "file
    # contains the call and lacks the gate" reads FALSE while `acquire_texture`
    # is still ungated -- reporting "fixed" when it is not.
    local fn_body
    fn_body=$(awk '
        /^[[:space:]]*(unsafe )?fn acquire_texture\(/ { inside=1; print; next }
        inside && /^[[:space:]]{4}(pub(\([^)]*\))? )?(unsafe )?fn / { exit }
        inside { print }
    ' "$surface")

    if [[ -z "$fn_body" ]]; then
        verdict=inconclusive
        say "- **$label — INCONCLUSIVE.** Could not locate \`acquire_texture\` in"
        say "  \`$surface\`. Read it by hand."
        return
    fi

    local reaches_appkit=0 gated=0
    grep -qE 'hosting_window|occlusionState|msg_send!\[[^]]*window' <<<"$fn_body" && reaches_appkit=1
    grep -q 'MainThreadMarker' <<<"$fn_body" && gated=1

    if (( reaches_appkit && ! gated )); then
        verdict=not-met
        say "- **$label — condition NOT met.** \`acquire_texture\` still messages AppKit UI"
        say "  objects from the calling thread with no main-thread gate. macOS stays on the"
        say "  inline raster lane; ADR-0045 decision 1 is unchanged."
        return
    fi

    verdict=changed
    if (( ! reaches_appkit )); then
        say "- **$label — CHANGED.** \`acquire_texture\` no longer contains the AppKit access"
        say "  decision 1 rests on. The reopen condition's first disjunct may be met."
    else
        say "- **$label — CHANGED.** \`acquire_texture\` still reaches an AppKit object but now"
        say "  also mentions \`MainThreadMarker\`; it may have been gated the way"
        say "  \`display_hdr_info\` was."
    fi
    say "  Read \`$surface\` before acting. This is a grep, not a proof."
}

say "## macOS raster reopen probe (issue #653)"
say ""

# --- explicit path: exercise the classifier against a fixture ----------------
if (( $# > 0 )); then
    classify "$1" "source: $1"
    say ""
    say "Advisory. Tracking: https://github.com/vanyastaff/flui/issues/653"
    exit 0
fi

# --- every wgpu-hal the resolution actually contains -------------------------
# Deliberately NOT "the newest one": picking a max meant comparing version
# strings, and `sort_by(.version)` in jq is lexicographic, so 30.0.10 sorts
# before 30.0.2. Probing all of them removes the selection problem and reports
# strictly more; a duplicated wgpu-hal is itself worth seeing.
meta=$(cargo metadata --format-version 1 2>/dev/null)
mapfile -t entries < <(jq -r '.packages[] | select(.name=="wgpu-hal") | "\(.version)\t\(.manifest_path)"' <<<"$meta" 2>/dev/null)

if (( ${#entries[@]} == 0 )); then
    say "- **INCONCLUSIVE.** Could not resolve \`wgpu-hal\` from \`cargo metadata\`."
    say ""
    say "Advisory. Tracking: https://github.com/vanyastaff/flui/issues/653"
    exit 0
fi

probed=()
for entry in "${entries[@]}"; do
    ver="${entry%%$'\t'*}"
    manifest="${entry#*$'\t'}"
    probed+=("$ver")
    classify "$(dirname "$manifest")/src/metal/surface.rs" "wgpu-hal $ver"
done

# --- what this run could NOT reach ------------------------------------------
# `cargo update` does not cross a semver major, and the workspace pins
# `wgpu = "30.0"`. A fix landing in the next major is invisible to everything
# above -- and that is a likely shape for it to arrive in, so say so rather
# than let a quiet run read as "nothing has changed upstream".
say ""
latest=$(curl -fsSL --max-time 20 \
    -H 'User-Agent: flui-ci (https://github.com/vanyastaff/flui)' \
    https://crates.io/api/v1/crates/wgpu-hal 2>/dev/null \
    | jq -r '.crate.max_stable_version // empty' 2>/dev/null)

if [[ -z "$latest" ]]; then
    say "_Could not reach crates.io to check for a release beyond this resolution._"
elif printf '%s\n' "${probed[@]}" | grep -qx "$latest"; then
    say "_crates.io's newest stable \`wgpu-hal\` is ${latest}, which is in the set probed above._"
else
    say "### Out of this probe's reach: wgpu-hal ${latest}"
    say ""
    say "crates.io publishes \`wgpu-hal\` ${latest}; this run probed ${probed[*]}."
    say "\`cargo update\` does not cross a semver major and the workspace pins"
    say "\`wgpu = \"30.0\"\`, so nothing above inspected it. **Read ${latest}'s"
    say "\`metal/surface.rs\` by hand before concluding the condition is still unmet.**"
fi

say ""
say "Advisory — never a gate. It also cannot see the reopen condition's second"
say "disjunct (an upstream *statement* that the off-thread access is safe), which"
say "is prose. A quiet run means \"the code still does it\", never \"the condition"
say "is still unmet\". Tracking: https://github.com/vanyastaff/flui/issues/653"
exit 0
