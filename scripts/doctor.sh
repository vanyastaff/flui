#!/usr/bin/env bash
# `just doctor`: report every tool the local gates need, with the command that
# installs each missing one. Runs under the stock macOS /bin/bash 3.2 on
# purpose -- it is what a fresh Mac has before anything else is installed --
# so no bash-4 syntax in this file.
#
#   just doctor         checks what `just ci` needs; exit 1 if any is missing
#   just doctor full    also what `just ci-full` needs; exit 1 if any is missing
#
# It only looks. It installs nothing and changes no configuration.
# Without `just` installed yet, run it as: bash scripts/doctor.sh [full]
set -uo pipefail

mode="${1:-ci}"
case "$mode" in ci|full) ;; *) echo "usage: $0 [ci|full]" >&2; exit 64 ;; esac

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
# shellcheck source=lib/interpreters.sh
source "$here/lib/interpreters.sh"

os="$(uname -s)"
missing_required=0
missing_optional=0

# row <scope> <name> <status> <detail> [install]
#   scope: ci (needed by `just ci`) | full (only `just ci-full`) | info
row() {
    local scope="$1" name="$2" status="$3" detail="$4" install="${5:-}"
    printf '  %-5s %-34s %-8s %s\n' "$scope" "$name" "$status" "$detail"
    if [ "$status" != ok ] && [ -n "$install" ]; then
        printf '  %-5s %-34s %-8s   install: %s\n' "" "" "" "$install"
    fi
    if [ "$status" != ok ]; then
        if [ "$scope" = ci ] || { [ "$scope" = full ] && [ "$mode" = full ]; }; then
            missing_required=$((missing_required + 1))
        else
            missing_optional=$((missing_optional + 1))
        fi
    fi
}

brew_or() {  # the install hint for this host: brew on macOS, the given one elsewhere
    if [ "$os" = Darwin ]; then echo "brew install $1"; else echo "$2"; fi
}

# check_bin <scope> <binary> <install> [version-args]
check_bin() {
    local scope="$1" bin="$2" install="$3" vargs="${4:---version}" version
    if command -v "$bin" >/dev/null 2>&1; then
        version="$("$bin" $vargs 2>/dev/null | head -1)"
        row "$scope" "$bin" ok "${version:-present}"
    else
        row "$scope" "$bin" MISSING "not on PATH" "$install"
    fi
}

# check_cargo_sub <scope> <subcommand> <install>
check_cargo_sub() {
    local scope="$1" sub="$2" install="$3" version
    if version="$(cargo "$sub" --version 2>/dev/null | head -1)" && [ -n "$version" ]; then
        row "$scope" "cargo $sub" ok "$version"
    else
        row "$scope" "cargo $sub" MISSING "not installed" "$install"
    fi
}

# check_target <scope> <triple>
check_target() {
    local scope="$1" triple="$2"
    if rustup target list --installed 2>/dev/null | grep -qx "$triple"; then
        row "$scope" "target $triple" ok "installed"
    else
        row "$scope" "target $triple" MISSING "rustup target not installed" "rustup target add $triple"
    fi
}

echo "flui doctor ($mode) -- $os, repo $root"
printf '  %-5s %-34s %-8s %s\n' scope name status detail

# --- interpreters -----------------------------------------------------------
if b="$(flui_find_bash4)"; then
    row ci "bash >= 4" ok "$b ($("$b" -c 'echo "$BASH_VERSION"'))"
else
    row ci "bash >= 4" MISSING "only bash ${BASH_VERSION} (the shell gates need mapfile)" "$(brew_or bash 'apt-get install bash')"
fi
if p="$(flui_find_python311)"; then
    row ci "python >= 3.11" ok "$p ($("$p" --version 2>&1))"
else
    row ci "python >= 3.11" MISSING "$(python3 --version 2>&1 || echo 'no python3') (tomllib needs 3.11)" "$(brew_or python@3.12 'apt-get install python3')"
fi

# --- `just ci` ----------------------------------------------------------------
check_bin ci cargo "https://rustup.rs (then: rustup show, in this repo, installs the pinned toolchain)"
check_bin ci just "cargo install --locked just"
check_cargo_sub ci nextest "cargo install --locked cargo-nextest"
check_bin ci typos "cargo install --locked typos-cli"
check_bin ci taplo "cargo install --locked taplo-cli"

# --- `just ci-full` -------------------------------------------------------------
check_cargo_sub full hack "cargo install --locked cargo-hack"
check_cargo_sub full deny "cargo install --locked cargo-deny"
check_bin full wasm-tools "$(brew_or wasm-tools 'cargo install --locked wasm-tools')"
want_wb="$(bash "$here/locked-version.sh" wasm-bindgen 2>/dev/null || true)"
have_wb="$(wasm-bindgen --version 2>/dev/null | cut -d' ' -f2 || true)"
if [ -n "$want_wb" ] && [ "$have_wb" = "$want_wb" ]; then
    row full "wasm-bindgen-cli" ok "$have_wb (= Cargo.lock)"
elif [ -n "$want_wb" ]; then
    row full "wasm-bindgen-cli" MISSING "have ${have_wb:-none}, Cargo.lock pins $want_wb (the runner refuses a mismatch)" \
        "cargo install --locked wasm-bindgen-cli --version $want_wb   (just wasm-test also does this)"
else
    row full "wasm-bindgen-cli" MISSING "the locked version is read with Python >= 3.11 (see above)" \
        "fix Python first; then: cargo install --locked wasm-bindgen-cli --version <Cargo.lock's wasm-bindgen>"
fi
check_bin full actionlint "$(brew_or actionlint 'go install github.com/rhysd/actionlint/cmd/actionlint@latest')"
check_bin full zizmor "$(brew_or zizmor 'cargo install --locked zizmor')"
check_target full wasm32-unknown-unknown
for t in x86_64-pc-windows-msvc aarch64-apple-darwin aarch64-linux-android aarch64-apple-ios; do
    check_target full "$t"
done
if rustup run nightly cargo miri --version >/dev/null 2>&1; then
    row full "nightly + miri" ok "$(rustup run nightly cargo miri --version 2>/dev/null | head -1)"
else
    row full "nightly + miri" MISSING "no nightly toolchain with miri" "rustup toolchain install nightly --component miri"
fi
msrv="$(sed -n 's/^rust-version *= *"\([0-9.]*\)".*/\1/p' "$root/Cargo.toml" | head -1)"
if [ -n "$msrv" ] && rustup toolchain list 2>/dev/null | grep -q "^$msrv"; then
    row full "toolchain $msrv (MSRV)" ok "installed"
else
    row full "toolchain ${msrv:-?} (MSRV)" MISSING "the msrv job's toolchain" "rustup toolchain install ${msrv:-<rust-version>}"
fi

# --- informational: jobs no local recipe can reproduce on this host -----------
if [ "$os" = Linux ]; then
    check_bin info xvfb-run "apt-get install xvfb   (flui-platform tests, live-smoke)" "-h"
fi
check_cargo_sub info sweep "cargo install --locked cargo-sweep   (just clean-stale)"

echo
if [ "$missing_required" -gt 0 ]; then
    echo "doctor: $missing_required missing for \`just $([ "$mode" = full ] && echo ci-full || echo ci)\` -- install commands above."
    exit 1
fi
if [ "$missing_optional" -gt 0 ]; then
    echo "doctor: everything \`just ci\` needs is here; $missing_optional optional item(s) missing (\`just doctor full\` for ci-full)."
else
    echo "doctor: everything \`just $([ "$mode" = full ] && echo ci-full || echo ci)\` needs is here."
fi
exit 0
