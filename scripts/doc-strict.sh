#!/usr/bin/env bash
# The strict rustdoc gate: the whole workspace, private items included, with
# every crate's `testing` feature ON.
#
# Why the feature list: a crate's test-support module is compiled only under
# `#[cfg(any(test, feature = "testing"))]`, so whether `cargo doc` renders it
# depends on which edge in the workspace happens to turn the feature on.
# `flui-testing` activates `flui-interaction/testing` and
# `flui-painting/testing` on NORMAL dependency edges, so those two are
# reached by a plain `cargo doc --workspace`; `flui-rendering`, `flui-layer`,
# and `flui-widgets` activate theirs only through self- and downstream
# `[dev-dependencies]`, which `cargo doc` ignores, so those three modules were
# never rendered and a broken intra-doc link in them was invisible to the
# gate. Passing the list explicitly makes coverage independent of which edge
# exists; deriving it from `cargo metadata` rather than writing it here means
# a crate that grows a `testing` feature is covered the day it does.
#
# `just doc-strict` and CI's `doc` job both run this file — one command, one
# environment, no mirror to drift.
set -euo pipefail

cd "$(dirname "$0")/.."

testing_features=$(
    cargo metadata --no-deps --format-version 1 --locked \
        | python3 -c '
import json, sys
packages = json.load(sys.stdin)["packages"]
print(",".join(p["name"] + "/testing" for p in packages if "testing" in p["features"]))
'
)

echo "doc-strict: testing features on: ${testing_features:-<none>}"

RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked \
    --document-private-items --features "$testing_features"
