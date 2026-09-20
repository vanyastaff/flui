#!/usr/bin/env python3
"""Run isolated AppKit show-mode cases with bounded, reaped child processes."""
import argparse
import pathlib
import plistlib
import shutil
import subprocess
import sys
import tempfile

CASES = ("normal", "hidden", "minimized", "maximized", "maximized-minimized", "fullscreen")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("cases", nargs="*", choices=CASES)
    parser.add_argument("--legacy-restore", action="store_true",
                        help="replay restore+activate to expose its mode-changing behavior")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise SystemExit("requires macOS with an active GUI session")
    with tempfile.TemporaryDirectory(prefix="flui-window-show-") as directory:
        contents = pathlib.Path(directory) / "WindowShow.app" / "Contents"
        binary = contents / "MacOS" / "window_show_probe"
        binary.parent.mkdir(parents=True)
        shutil.copy2(args.binary.resolve(), binary)
        with (contents / "Info.plist").open("wb") as stream:
            plistlib.dump({"CFBundlePackageType": "APPL", "CFBundleName": "WindowShow",
                          "CFBundleIdentifier": "dev.flui.window-show-probe",
                          "CFBundleExecutable": "window_show_probe",
                          "NSPrincipalClass": "NSApplication"}, stream)
        for case in args.cases or CASES:
            command = [str(binary), case]
            if args.legacy_restore:
                command.append("--legacy-restore")
            # subprocess.run kills and reaps its child on timeout. There is no
            # success process::exit in the probe: native run must return normally.
            result = subprocess.run(command, capture_output=True, text=True, timeout=12)
            print(result.stdout, end="", flush=True)
            print(result.stderr, end="", file=sys.stderr, flush=True)
            if result.returncode or f"SHOW_MODE_VERIFIED={case}" not in result.stdout or "SHOW_NORMAL_RETURN" not in result.stdout:
                raise RuntimeError(f"{case} failed: exit {result.returncode}")
            print(f"PASS {case}", flush=True)


if __name__ == "__main__":
    main()
