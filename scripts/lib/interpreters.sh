#!/usr/bin/env bash
# Interpreter floors for the repo's scripts, in one place.
#
# Two scripts' needs are above what a stock macOS ships: `mapfile` needs
# bash >= 4 (/bin/bash is 3.2), and `tomllib` needs Python >= 3.11
# (/usr/bin/python3 is 3.9). Linux CI has both, so without a check a Mac
# contributor gets a confusing mid-script error (`mapfile: command not found`
# swallowed into an unbound-variable abort, `ModuleNotFoundError: tomllib`).
#
# Two uses:
#   source "$(dirname "${BASH_SOURCE[0]}")/lib/interpreters.sh"
#   flui_require_bash4 "$0"            # exit 2 with an install hint on bash < 4
#   flui_require_python311 "$0"        # sets FLUI_PYTHON, or exit 2 with a hint
# and, from the justfile, to pick interpreters once for every recipe:
#   bash scripts/lib/interpreters.sh bash     # prints a bash >= 4, else "bash"
#   bash scripts/lib/interpreters.sh python   # prints a python >= 3.11, else "python3"
# The fallbacks print the plain name on purpose: the script it then runs hits
# its own guard and says what to install, instead of the justfile failing.
#
# Must itself run under bash 3.2: no bash-4 syntax in this file.

flui_find_bash4() {
    local candidate
    for candidate in bash /opt/homebrew/bin/bash /usr/local/bin/bash; do
        command -v "$candidate" >/dev/null 2>&1 || continue
        # shellcheck disable=SC2016 # the inner shell expands it
        if [ "$("$candidate" -c 'echo "${BASH_VERSINFO[0]}"' 2>/dev/null)" -ge 4 ] 2>/dev/null; then
            command -v "$candidate"
            return 0
        fi
    done
    return 1
}

flui_find_python311() {
    local candidate
    for candidate in python3 python3.14 python3.13 python3.12 python3.11 python; do
        command -v "$candidate" >/dev/null 2>&1 || continue
        if "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2>/dev/null; then
            command -v "$candidate"
            return 0
        fi
    done
    return 1
}

flui_require_bash4() {
    if [ "${BASH_VERSINFO[0]:-0}" -ge 4 ]; then
        return 0
    fi
    {
        echo "${1:-this script}: needs bash >= 4 (it uses mapfile); running under bash ${BASH_VERSION:-unknown}."
        echo "  macOS ships bash 3.2 as /bin/bash. Install a current one: brew install bash"
        echo "  then run it through just (recipes pick the newest bash on PATH) or as: /opt/homebrew/bin/bash ${1:-<script>}"
        echo "  \`just doctor\` checks every tool the gate needs."
    } >&2
    exit 2
}

flui_require_python311() {
    if FLUI_PYTHON=$(flui_find_python311); then
        export FLUI_PYTHON
        return 0
    fi
    {
        echo "${1:-this script}: needs Python >= 3.11 (it uses tomllib); found $(python3 --version 2>&1 || echo 'no python3')."
        echo "  macOS: brew install python@3.12 (it installs python3.12; this check finds it by that name)."
        echo "  \`just doctor\` checks every tool the gate needs."
    } >&2
    exit 2
}

# Executed (not sourced): print the chosen interpreter for the justfile.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    case "${1:-}" in
        bash) flui_find_bash4 || echo bash ;;
        python) flui_find_python311 || echo python3 ;;
        *) echo "usage: $0 bash|python" >&2; exit 64 ;;
    esac
fi
