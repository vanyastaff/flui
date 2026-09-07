#!/usr/bin/env python3
"""Print the crates that opt in to executing wasm32 tests, one per line.

The opt-in signal is a `wasm-bindgen-test` entry under a wasm32-conditional
`dev-dependencies` table. That is where a contributor already declares intent,
so it cannot drift from the code the way a hardcoded list in the justfile and
the CI workflow would.

Parsed rather than grepped. A substring search over the manifest also matches a
comment, a normal dependency, or a non-wasm32 target table -- none of which mean
"this crate has wasm tests" -- and the resulting mismatch between the check and
its documented contract would be silent, which is the failure mode this whole
harness exists to remove.
"""

import sys
import tomllib
from pathlib import Path

DEP = "wasm-bindgen-test"


def opts_in(manifest: Path) -> bool:
    with manifest.open("rb") as handle:
        parsed = tomllib.load(handle)
    for key, table in (parsed.get("target") or {}).items():
        if "wasm32" not in key:
            continue
        if DEP in (table.get("dev-dependencies") or {}):
            return True
    return False


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "crates")
    for manifest in sorted(root.glob("*/Cargo.toml")):
        try:
            if opts_in(manifest):
                print(manifest.parent.name)
        except (OSError, tomllib.TOMLDecodeError) as exc:
            print(f"{manifest}: {exc}", file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
