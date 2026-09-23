#!/usr/bin/env bash
# Which workspace packages a change touches, and the cargo arguments that
# select them -- the ONE computation behind CI's fast lane (the `plan` job)
# and `just check-changed`, so the two cannot disagree. The logic lives in
# scripts/lib/change_scope.py; this wrapper only finds a Python for it
# (any python3 >= 3.9 works: it needs no tomllib).
#
#   scripts/affected-crates.sh                         # vs origin/main, human summary
#   scripts/affected-crates.sh --worktree              # also uncommitted + untracked files
#   scripts/affected-crates.sh --base <sha> --format github >> "$GITHUB_OUTPUT"
#   eval "$(scripts/affected-crates.sh --worktree --format shell)"
#
# Modes: docs (only documentation), none (only repo tooling), packages (the
# changed packages + every workspace package depending on them, transitively,
# through normal/dev/build edges), full (a workspace-wide input such as
# Cargo.lock or a workflow changed, or a file no package owns).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/interpreters.sh
source "$here/lib/interpreters.sh"
python="$(flui_find_python311 || command -v python3 || echo python3)"
exec "$python" -B "$here/lib/change_scope.py" "$@"
