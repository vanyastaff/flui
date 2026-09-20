#!/usr/bin/env python3
"""Run the owned-delegate protocol probe on an explicitly selected booted simulator.

Requires the ios_execution_probe example built for that simulator architecture.
This invokes delegate methods, not OS background transitions. Marker completion
is not normal UIApplicationMain return; the runner terminates its owned probe.
"""
import argparse
import json
import pathlib
import plistlib
import shutil
import subprocess
import tempfile
import time


def command(*args, timeout=30):
    return subprocess.run(args, check=True, text=True, capture_output=True, timeout=timeout).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("udid")
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("--case", choices=["transient", "foreground-reentered-background", "background-reentered-foreground", "duplicate-foreground", "foreground-callback-panic", "background-nested-active", "background-nested-inactive", "foreground-nested-active", "foreground-nested-inactive", "nested-foreground-then-panic", "superseded-foreground", "background-nested-run-loop"], default="transient")
    options = parser.parse_args()
    inventory = json.loads(command("xcrun", "simctl", "list", "devices", "--json"))
    selected = [device for devices in inventory["devices"].values() for device in devices
                if device["udid"] == options.udid and device.get("isAvailable")]
    if len(selected) != 1 or selected[0]["state"] != "Booted":
        raise RuntimeError("exact selected simulator must already be available and Booted")
    identifier = "dev.flui.execution-probe"
    with tempfile.TemporaryDirectory(prefix="flui-ios-execution-") as directory:
        bundle = pathlib.Path(directory) / "ExecutionProbe.app"
        bundle.mkdir()
        shutil.copy2(options.binary, bundle / "probe")
        with (bundle / "Info.plist").open("wb") as file:
            plistlib.dump({"CFBundleIdentifier": identifier, "CFBundleExecutable": "probe",
                          "CFBundleName": "ExecutionProbe", "CFBundlePackageType": "APPL",
                          "CFBundleVersion": "1", "CFBundleShortVersionString": "1.0.0",
                          "MinimumOSVersion": "14.0", "LSRequiresIPhoneOS": True,
                          "UIDeviceFamily": [1, 2], "UILaunchScreen": {}}, file)
        command("xcrun", "simctl", "install", options.udid, str(bundle), timeout=120)
        container = pathlib.Path(command("xcrun", "simctl", "get_app_container", options.udid, identifier, "data"))
        result = container / "tmp/flui-execution-result.txt"
        result.unlink(missing_ok=True)
        launched = False
        try:
            launched = True
            print(command("xcrun", "simctl", "launch", options.udid, identifier, options.case, timeout=90), flush=True)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if result.exists():
                    text = result.read_text()
                    print(text, flush=True)
                    return 0 if text.startswith("PASS ") else 1
                time.sleep(0.1)
            raise TimeoutError("owned probe produced no protocol result in 30 seconds")
        finally:
            if launched:
                subprocess.run(["xcrun", "simctl", "terminate", options.udid, identifier], check=False, timeout=30)


if __name__ == "__main__":
    raise SystemExit(main())
