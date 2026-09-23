#!/usr/bin/env python3
"""Run a built AppKit probe with an outer process deadline and return/drop oracles."""
import pathlib
import plistlib
import shutil
import subprocess
import sys
import tempfile


def main():
    if sys.platform != "darwin":
        raise SystemExit("AppKit exit checks require macOS with an active GUI session")
    executable = pathlib.Path(sys.argv[1]).resolve()
    cases = sys.argv[2:] or ["last-window", "two-windows", "veto", "service-release",
                            "hook-reentrant", "close-reopens", "hook-opens", "explicit-quit",
                            "native-quit", "bootstrap-quit", "pre-run-quit", "bootstrap-error",
                            "foreign-owner", "running-owner", "delegate-replaced", "policy-replaced",
                            "bootstrap-panic", "quit-panic", "coalesced", "hook-drop-panic"]
    with tempfile.TemporaryDirectory(prefix="flui-exit-probe-") as directory:
        contents = pathlib.Path(directory) / "ExitPolicyProbe.app" / "Contents"
        binary = contents / "MacOS" / "exit_policy_probe"
        binary.parent.mkdir(parents=True)
        shutil.copy2(executable, binary)
        with (contents / "Info.plist").open("wb") as stream:
            plistlib.dump({"CFBundlePackageType": "APPL", "CFBundleName": "ExitPolicyProbe",
                          "CFBundleIdentifier": "dev.flui.exit-policy-probe",
                          "CFBundleExecutable": "exit_policy_probe",
                          "NSPrincipalClass": "NSApplication"}, stream)
        for case in cases:
            try:
                result = subprocess.run([str(binary), case], capture_output=True, text=True, timeout=8)
            except subprocess.TimeoutExpired as error:
                raise SystemExit(f"FAIL {case}: native run did not return within 8 seconds: {error.stdout!r}") from error
            print(result.stdout, end="")
            print(result.stderr, end="", file=sys.stderr)
            markers = ["EXIT_POLICY_RETURNED", "EXIT_POLICY_STACK_DROPPED", f"EXIT_POLICY_CASE={case}"]
            if case != "foreign-owner":
                markers += ["EXIT_POLICY_QUIT_NOTIFIED", "EXIT_POLICY_CALLBACK_DROPPED", "EXIT_POLICY_NATIVE_RELEASED"]
            if case in ("last-window", "two-windows", "veto", "service-release", "hook-reentrant", "close-reopens", "hook-opens"):
                markers += ["EXIT_POLICY_WINDOW_CLOSED"]
            if case in ("close-reopens", "hook-opens"):
                markers += ["EXIT_POLICY_REPLACEMENT_SURVIVED"]
            if case == "hook-reentrant":
                markers += ["EXIT_POLICY_REPLACEMENT_HOOK_INVOKED"]
            if case == "coalesced":
                markers += ["EXIT_POLICY_BURST_COALESCED"]
            if case == "hook-drop-panic":
                markers += ["EXIT_POLICY_HOOK_DROP_ARMED"]
            if result.returncode or not all(marker in result.stdout for marker in markers):
                raise SystemExit(f"FAIL {case}: return/drop/lifecycle oracles missing or process failed ({result.returncode})")
            print(f"PASS {case}")


if __name__ == "__main__":
    main()
