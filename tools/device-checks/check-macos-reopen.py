#!/usr/bin/env python3
"""Bound delegate routing and actual reopen AppleEvents in the original process."""
import pathlib
import plistlib
import selectors
import shutil
import subprocess
import sys
import tempfile
import time

CASES = ["visible", "empty", "starting", "nested", "replacement", "before-delivery", "quit-inside",
         "quit-pending", "proxy-quit-pending", "panic", "drop-reentry", "drop-panic", "stale", "os-visible", "os-empty"]


def run_case(binary, bundle, case):
    process = subprocess.Popen([str(binary), case], stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True)
    prefix = ""
    try:
        if case.startswith("os-"):
            deadline = time.monotonic() + 6
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not selector.select(remaining):
                        raise RuntimeError("native process did not announce readiness")
                    line = process.stdout.readline()
                    if not line:
                        raise RuntimeError("native process ended before readiness")
                    prefix += line
                    if line.startswith("REOPEN_READY_PID="):
                        assert int(line.strip().split("=")[1]) == process.pid
                        break
            assert process.poll() is None, "original process must still be running"
            # No -n: ask LaunchServices to reopen this exact running bundle.
            subprocess.run(["/usr/bin/open", "-a", str(bundle)], check=True,
                           capture_output=True, text=True, timeout=3)
        stdout, stderr = process.communicate(timeout=8)
    except (subprocess.TimeoutExpired, RuntimeError, AssertionError, subprocess.CalledProcessError):
        process.kill()
        stdout, stderr = process.communicate()
        print(prefix + stdout, end="")
        print(stderr, end="", file=sys.stderr)
        raise
    stdout = prefix + stdout
    print(stdout, end="")
    print(stderr, end="", file=sys.stderr)
    markers = [f"REOPEN_RETURNED_PID={process.pid}", f"REOPEN_CASE={case}"]
    if case not in ("quit-pending", "proxy-quit-pending", "drop-reentry", "drop-panic", "before-delivery"):
        markers += ["REOPEN_DELIVERED=0"]
    if case in ("nested", "panic"):
        markers += ["REOPEN_DELIVERED=1"]
    if case in ("nested", "replacement", "quit-inside", "drop-reentry", "drop-panic"):
        markers += ["REOPEN_NESTED_PUMP_RETURNED"]
    if case == "before-delivery":
        markers += ["REOPEN_LATEST_REGISTRATION"]
    if case == "replacement":
        markers += ["REOPEN_REPLACEMENT"]
    if case in ("nested", "replacement", "drop-reentry", "drop-panic"):
        markers += ["REOPEN_SERIAL_DEPTH_VERIFIED"]
    if case in ("drop-reentry", "drop-panic"):
        markers += ["REOPEN_DROP_REPLACEMENT", "REOPEN_CAPTURE_CLEANUP_VERIFIED"]
    if case == "stale":
        markers += ["REOPEN_STALE_INERT"]
    if process.returncode or not all(marker in stdout for marker in markers):
        raise RuntimeError(f"FAIL {case}: code {process.returncode}, missing native oracles")
    transport = "OS AppleEvent/original PID" if case.startswith("os-") else "direct delegate"
    print(f"PASS {case} ({transport})")


def main():
    if sys.platform != "darwin":
        raise SystemExit("AppKit reopen checks require macOS with an active GUI session")
    executable = pathlib.Path(sys.argv[1]).resolve()
    cases = sys.argv[2:] or CASES
    if any(case not in CASES for case in cases):
        raise SystemExit("unknown reopen case")
    with tempfile.TemporaryDirectory(prefix="flui-reopen-probe-") as directory:
        bundle = pathlib.Path(directory) / "ReopenProbe.app"
        contents = bundle / "Contents"
        binary = contents / "MacOS" / "reopen_probe"
        binary.parent.mkdir(parents=True)
        shutil.copy2(executable, binary)
        with (contents / "Info.plist").open("wb") as stream:
            plistlib.dump({"CFBundlePackageType": "APPL", "CFBundleName": "ReopenProbe",
                          "CFBundleIdentifier": "dev.flui.reopen-probe",
                          "CFBundleExecutable": "reopen_probe",
                          "NSPrincipalClass": "NSApplication"}, stream)
        for case in cases:
            run_case(binary, bundle, case)


if __name__ == "__main__":
    main()
