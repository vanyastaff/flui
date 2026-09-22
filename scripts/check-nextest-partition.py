#!/usr/bin/env python3
"""Check that .config/nextest.toml's nested-cargo filter is spelled identically
in its three places.

`just test-ci` runs the suite as two stages: profile `no-nested-cargo`, then
profile `nested-cargo`. Their default filters must be exact complements, and
the timeout override must name the same tests; nextest has no way to name a
filter once and reuse it, so the same expression is written three times:

    [[profile.default.overrides]]  filter         = 'F'
    [profile.no-nested-cargo]      default-filter = 'not (F)'
    [profile.nested-cargo]         default-filter = 'F'

A test added to one copy and not the others either loses its time budget or
silently drops out of `just test-ci` (neither stage selects it). This compares
the three strings; it does not evaluate them (that needs built test binaries --
`cargo nextest list` per profile, see docs/testing.md).
"""
import sys

if sys.version_info < (3, 11):  # tomllib; macOS /usr/bin/python3 is 3.9
    print(
        f"{sys.argv[0]}: needs Python >= 3.11 (it uses tomllib); running "
        f"{sys.version.split()[0]}. macOS: brew install python@3.12, then run it "
        "through just (recipes pick a Python >= 3.11). `just doctor` checks every tool.",
        file=sys.stderr,
    )
    sys.exit(2)  # same code as the shell scripts' interpreter guards
import tomllib
from pathlib import Path

CONFIG = Path(__file__).resolve().parent.parent / ".config" / "nextest.toml"


def main() -> int:
    config = tomllib.loads(CONFIG.read_text(encoding="utf-8"))
    profiles = config.get("profile", {})
    try:
        nested = profiles["nested-cargo"]["default-filter"]
        rest = profiles["no-nested-cargo"]["default-filter"]
    except KeyError as missing:
        print(f"nextest-partition: profile or key {missing} missing from {CONFIG}", file=sys.stderr)
        return 1
    overrides = [o.get("filter") for o in profiles.get("default", {}).get("overrides", [])]
    problems = []
    if rest != f"not ({nested})":
        problems.append(
            "profile.no-nested-cargo.default-filter is not exactly `not (<nested-cargo filter>)`:\n"
            f"    nested-cargo:    {nested}\n    no-nested-cargo: {rest}"
        )
    if nested not in overrides:
        problems.append(
            "no [[profile.default.overrides]] entry has the nested-cargo filter verbatim, so the "
            "group's slow-timeout budget no longer covers exactly these tests"
        )
    if problems:
        for problem in problems:
            print(f"nextest-partition: {problem}", file=sys.stderr)
        return 1
    print("nextest-partition: nested-cargo filter identical in the override and both profiles")
    return 0


if __name__ == "__main__":
    sys.exit(main())
