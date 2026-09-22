#!/usr/bin/env python3
"""Build the sole-facade safe-area fixture and run its bounded native check.

Requires an explicitly selected, already booted arm64 simulator. The fixture
mounts a real application through `flui` alone, wraps half of a Stack in
`SafeArea`, and reports the ambient MediaQuery padding alongside both laid-out
geometries; the runner launches it and reads that marker.

The fixture consumes the framework's own iOS runner, so a pass covers the whole
path: UIKit's `safeAreaInsets` -> the window's owner-thread sample -> the
addressed presentation's `MediaQuery` -> `SafeArea`'s padding. It is a layout
check on the selected simulator, not a claim about any other device class.
"""
import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("udid")
    parser.add_argument("directory", type=pathlib.Path)
    parser.add_argument("--target-dir", type=pathlib.Path, default=ROOT / "target")
    options = parser.parse_args()
    directory = options.directory.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "src").mkdir(exist_ok=True)
    shutil.copy2(ROOT / "tests/fixtures/ios_safe_area.rs", directory / "src/main.rs")
    (directory / "Cargo.toml").write_text(
        '[workspace]\n[package]\nname="flui-ios-safe-area"\nversion="0.0.0"\nedition="2024"\n'
        '[dependencies]\nflui={path=' + json.dumps(str(ROOT)) + '}\n'
        'objc2="0.6.4"\ndispatch2="0.3.1"\n'
        'objc2-ui-kit={version="0.3.2",default-features=false,features=['
        '"UIApplication","UIScene","UISceneSession","UIResponder",'
        '"UIView","UIViewController","UIWindow","UIWindowScene"]}\n'
        'objc2-foundation="0.3.2"\n')
    shutil.copy2(ROOT / "Cargo.lock", directory / "Cargo.lock")
    # Apple's python3 injects SDKROOT pointing at the Command Line Tools SDK.
    # When the installed CLT SDK is newer than the selected Xcode, the host
    # linker cannot read it and every build script fails to link. Plain
    # `cargo build` from a shell has no SDKROOT and resolves the SDK through
    # the selected Xcode, so drop the injected one instead of pinning a second.
    env = dict(os.environ, CARGO_PROFILE_DEV_DEBUG="0", CARGO_INCREMENTAL="0")
    env.pop("SDKROOT", None)
    target = options.target_dir.resolve()
    with (directory / "build.log").open("w") as log:
        subprocess.run(["cargo", "build", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
                        "--target", "aarch64-apple-ios-sim", "--target-dir", str(target)],
                       env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)
    binary = target / "aarch64-apple-ios-sim/debug/flui-ios-safe-area"
    return subprocess.run([sys.executable, str(ROOT / "scripts/check-ios-execution.py"),
                           options.udid, str(binary), "--case", "safe-area-layout"],
                          check=False, timeout=180).returncode


if __name__ == "__main__":
    raise SystemExit(main())
