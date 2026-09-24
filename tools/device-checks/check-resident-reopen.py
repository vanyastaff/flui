#!/usr/bin/env python3
"""Stage a sole-flui fixture; bound interactive close then actual OS reopen.

The operator uses CUA to click Increment and close the first window. The driver
sends only LaunchServices reopen after observing real input, present and dispose.
"""
import argparse
import json
import pathlib
import plistlib
import shutil
import subprocess
import sys
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]


def prepare(directory, target):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "src").mkdir(exist_ok=True)
    shutil.copy2(ROOT / "tests/fixtures/resident_app.rs", directory / "src/main.rs")
    (directory / "Cargo.toml").write_text(
        '[workspace]\n[package]\nname="flui-resident-probe"\nversion="0.0.0"\nedition="2024"\n'
        '[dependencies]\nflui={path=' + json.dumps(str(ROOT)) + '}\n'
        'tracing-subscriber={version="0.3",features=["fmt","env-filter"]}\n')
    build = subprocess.run(["cargo", "build", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
                            "--target-dir", str(target), "--message-format=json-render-diagnostics"],
                           capture_output=True, text=True, check=False)
    (directory / "build.log").write_text(build.stderr + build.stdout)
    if build.returncode:
        raise RuntimeError(f"build failed; see {directory / 'build.log'}")
    artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith('{')]
    executable = next(item["executable"] for item in artifacts
                      if item.get("reason") == "compiler-artifact" and item.get("executable")
                      and item["target"]["name"] == "flui-resident-probe")
    bundle = directory / "ResidentProbe.app"
    binary = bundle / "Contents/MacOS/resident_probe"
    binary.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(executable, binary)
    with (bundle / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump({"CFBundlePackageType": "APPL", "CFBundleName": "ResidentProbe",
                      "CFBundleIdentifier": "dev.flui.resident-probe", "CFBundleExecutable": "resident_probe",
                      "NSPrincipalClass": "NSApplication"}, stream)
    print(bundle, flush=True)


def run(directory, timeout, windowless):
    bundle = directory / "ResidentProbe.app"
    binary = bundle / "Contents/MacOS/resident_probe"
    events = []
    process = subprocess.Popen([str(binary)] + (["--windowless"] if windowless else []), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    log = (directory / "native.log").open("w")
    def collect():
        for line in process.stdout:
            events.append(line)
            log.write(line)
            log.flush()
            print(line, end="", flush=True)
    reader = threading.Thread(target=collect, daemon=True)
    reader.start()
    def wait_for(predicate, seconds, message):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            if process.poll() is not None:
                reader.join(timeout=1)
                if predicate():
                    return
                raise RuntimeError(f"original PID exited before {message}: {process.returncode}")
            time.sleep(0.05)
        raise RuntimeError(f"timeout: {message}")
    def presented_after_init(generation):
        snapshot = list(events)
        marker = f"RESIDENT_INIT={generation} "
        position = next((i for i, line in enumerate(snapshot) if marker in line), None)
        return position is not None and any("present_submitted" in x for x in snapshot[position + 1:])
    try:
        wait_for(lambda: any(f"RESIDENT_PID={process.pid}" in x for x in events)
                 and presented_after_init(1), 25, "first root and GPU present")
        print(f"RESIDENT_DRIVER_READY PID={process.pid} BUNDLE={bundle}\nCUA: click Increment, verify changed text, then close the window.", flush=True)
        wait_for(lambda: any("RESIDENT_INPUT=1" in x for x in events)
                 and any("RESIDENT_DISPOSE=1" in x for x in events), timeout, "real input and first-window disposal")
        assert process.poll() is None
        before = len(events)
        subprocess.run(["/usr/bin/open", "-a", str(bundle)], check=True, timeout=5)
        print(f"RESIDENT_OS_REOPEN_SENT PID={process.pid}", flush=True)
        wait_for(lambda: presented_after_init(2), 12, "same-process second rendered root")
        print("CUA: verify the reopened window, then click Increment.", flush=True)
        wait_for(lambda: any("RESIDENT_INPUT=2" in x for x in events[before:]), timeout, "second-window input")
        assert sum("RESIDENT_SERVICE_START=" in x for x in events) == 1
        if windowless:
            assert any("RESIDENT_WORKER_SHOW_ADMITTED" in x for x in events)
        assert any("RESIDENT_FACTORY=2 PERSISTENT=1" in x for x in events)
        assert any("RESIDENT_INIT=2 LOCAL=0" in x for x in events)
        assert any("RESIDENT_INPUT=2 LOCAL=1 PERSISTENT=2" in x for x in events)
        print("CUA: click Quit application; normal return is required.", flush=True)
        wait_for(lambda: any(f"RESIDENT_RETURNED_PID={process.pid}" in x for x in events), timeout, "normal application return")
        assert process.wait(timeout=5) == 0
        assert any("RESIDENT_SERVICE_CANCELLED" in x for x in events)
        assert any("RESIDENT_SERVICE_DROPPED" in x for x in events)
        assert sum("RESIDENT_FACTORY=" in x for x in events) == 2
        print("RESIDENT_REOPEN_RENDER_INPUT_NORMAL_RETURN_PASS", flush=True)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
            print("DRIVER_CLEANUP_SIGTERM (not normal-shutdown evidence)", flush=True)
        reader.join(timeout=2)
        log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["prepare", "run"])
    parser.add_argument("directory", type=pathlib.Path)
    parser.add_argument("--target-dir", type=pathlib.Path, default=ROOT / "target/cli-template-check")
    parser.add_argument("--windowless", action="store_true")
    parser.add_argument("--interaction-timeout", type=int, default=90)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("requires macOS with an active GUI session")
    if args.mode == "prepare":
        prepare(args.directory.resolve(), args.target_dir.resolve())
    else:
        run(args.directory.resolve(), args.interaction_timeout, args.windowless)


if __name__ == "__main__":
    main()
