#!/usr/bin/env python3
"""Drive the generated counter through macOS accessibility, as VoiceOver would.

The evidence collector behind docs/BETA.md's accessibility gap on macOS.
It builds `examples/a11y_probe.rs` (the CLI counter template's tree, with
the facade's `a11y` feature so the AccessKit adapter is installed), runs it
on a real window, and runs `scripts/macos-ax-client.swift` against the
process: an `AXUIElement` client — the API every macOS assistive
technology uses — that reads the window's accessibility tree, finds the
button by its label, performs `AXPress`, and reads the count back. No
pointer or keyboard event is synthesised anywhere; if the count advances,
a screen-reader user could press the button.

Exit 0 on PASS (button found, pressed, count read back as "1"); 1 on FAIL
(with both tree dumps on stdout); 2 when the host cannot take the
measurement (not macOS, swiftc missing, or this process not trusted for
accessibility under System Settings > Privacy & Security > Accessibility —
a denied query is not an inaccessible app, so nothing is decided).

Usage: python3 scripts/check-macos-a11y.py
"""

from __future__ import annotations

import os
import platform
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
BINARY = REPO_ROOT / "target" / "release" / "examples" / "a11y_probe"
CLIENT_SOURCE = REPO_ROOT / "scripts" / "macos-ax-client.swift"
CLIENT_BINARY = REPO_ROOT / "target" / "a11y" / "ax-client"
BUTTON_LABEL = "Increment"
EXPECTED_AFTER_PRESS = "1"
# Time for the window and the first frame; the client retries for 10 s more.
LAUNCH_SETTLE_SECONDS = 4.0
CLIENT_TIMEOUT_SECONDS = 60


def clean_env() -> dict:
    """Apple Python's injected SDKROOT breaks the linker for a child cargo
    (see check-macos-workload.py); drop it."""
    env = os.environ.copy()
    env.pop("SDKROOT", None)
    env.pop("DEVELOPER_DIR", None)
    return env


def main() -> int:
    if platform.system() != "Darwin":
        print(f"SKIP: check-macos-a11y.py is macOS-only (platform is {platform.system()})")
        return 0

    env = clean_env()
    build = [
        "cargo", "build", "--locked", "--release",
        "--example", "a11y_probe", "--features", "material,a11y",
    ]
    print(f"building: {' '.join(build)}")
    if subprocess.run(build, cwd=REPO_ROOT, env=env, check=False).returncode != 0:
        print("FAIL: build — cargo build did not succeed")
        return 1

    CLIENT_BINARY.parent.mkdir(parents=True, exist_ok=True)
    compile_client = ["xcrun", "swiftc", "-O", str(CLIENT_SOURCE), "-o", str(CLIENT_BINARY)]
    print(f"compiling: {' '.join(compile_client)}")
    compiled = subprocess.run(compile_client, cwd=REPO_ROOT, env=env, capture_output=True, text=True, check=False)
    if compiled.returncode != 0:
        print(compiled.stderr)
        print("CANNOT_VERIFY: the AX client did not compile (swiftc / Xcode toolchain)")
        return 2

    env["RUST_LOG"] = "warn"
    print(f"running: {BINARY}")
    probe = subprocess.Popen([str(BINARY)], cwd=REPO_ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    try:
        time.sleep(LAUNCH_SETTLE_SECONDS)
        if probe.poll() is not None:
            print(probe.stderr.read() if probe.stderr else "")
            print(f"FAIL: the probe exited with {probe.returncode} before the client could query it")
            return 1
        client = subprocess.run(
            [str(CLIENT_BINARY), str(probe.pid), BUTTON_LABEL, EXPECTED_AFTER_PRESS],
            capture_output=True, text=True, timeout=CLIENT_TIMEOUT_SECONDS, check=False,
        )
    finally:
        probe.terminate()
        try:
            probe.wait(timeout=10)
        except subprocess.TimeoutExpired:
            probe.kill()
    print(client.stdout, end="")
    if client.stderr.strip():
        print(client.stderr, end="", file=sys.stderr)
    if client.returncode == 2:
        print("A11Y=CANNOT_VERIFY")
        return 2
    verdict = "PASS" if client.returncode == 0 else "FAIL"
    print(f"A11Y={verdict}")
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
