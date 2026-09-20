#!/usr/bin/env python3
"""Verify a macOS application renders into its window under every launch route.

The launch route is not cosmetic. A direct launch from a shell, a Finder
double-click (`open`, i.e. LaunchServices) and a background launch
(`open -g`, LaunchServices without activation) differ in who starts the
process and whether it ever becomes frontmost, and this repository has carried
a recorded observation that a LaunchServices-launched application showed a
blank window while the same bundle rendered when started directly.

The oracle is therefore the **pixels of the application's own window**, not a
frame count and not process survival: a window can exist, hold a live frame
pump, and still show nothing — the iOS arm of `just ios-sim` records the same
lesson, where a UIKit layer repainted over the Metal layer while the app kept
logging frames. The capture is `screencapture -l <window number>`, taken by
window number rather than by screen region so an occluded or never-activated
window is still read from its own backing store; a full-screen grab would
photograph whatever is on top of it and turn the background arm into a
tautology.

That capture is the one part of this gate the host can revoke: reading another
application's window needs Screen Recording. The permission is preflighted
(`CGPreflightScreenCaptureAccess`) and its absence is reported as
CANNOT VERIFY, never as a blank window — without that distinction a denied
capture and the defect look identical.

The window number comes from `scripts/macos-window-list.swift`; see its header
for why enumerating windows needs a CoreGraphics caller and not `ctypes`.

Exit codes: 0 all launches rendered, 1 a launch was refused / produced no window
/ produced a window with nothing in it, 2 the host could not be measured
(Screen Recording not granted, or no `swiftc`). Two is a loud non-pass, never a
quiet one.
"""
import argparse
import ctypes
import ctypes.util
import os
import pathlib
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import time
from collections import Counter

ROOT = pathlib.Path(__file__).resolve().parents[1]
WINDOW_LIST = ROOT / "scripts/macos-window-list.swift"

# Arms, in the order they are reported. `direct` is the control: if it fails
# too, the launch route is not what broke.
ARMS = ("direct", "launchservices", "launchservices-background")

# How long a launched application gets to put a window on screen.
WINDOW_TIMEOUT = 20.0

# Colour buckets are 32-wide per channel, so antialiasing and gradients inside
# one flat fill do not inflate the count. A blank window is one bucket.
BUCKET = 32
# A window whose content region holds fewer distinct buckets than this is
# reported as flat. Four is deliberately low: it separates "something was
# drawn" from "nothing was drawn", and is not a claim about layout quality. Nor
# is the bucket count a claim about how *much* was drawn — a window that is
# 99.9 % one colour and 0.1 % text is drawn, and a reader who wants that
# distinction has it in the `ink=` figure printed for every launch. What the
# count does separate is the measured failure: an empty window is exactly one
# bucket, whatever the system background happens to be.
MIN_BUCKETS = 4

# When the caller names the colour the fixture paints, that colour is the
# oracle instead: a window that painted it is not blank whatever else it
# holds, and the window's background is a system colour, never the fixture's.
MIN_EXPECT_SHARE = 1.0

# Exit codes. A blank window and a host that could not be measured are not the
# same result and must not share one, or a revoked permission would be filed as
# a rendering defect — and, worse, a rendering defect would be filed as a
# permission problem and retried.
EXIT_PASS = 0
EXIT_BLANK = 1
EXIT_CANNOT_VERIFY = 2


def cannot_verify(reason):
    """Report a measurement that could not be taken, distinctly from a blank window."""
    print(f"MACOS_LAUNCH_RENDER=CANNOT_VERIFY: {reason}", file=sys.stderr)
    raise SystemExit(EXIT_CANNOT_VERIFY)


def screen_capture_permitted():
    """Whether this host may read another application's window contents."""
    core_graphics = ctypes.CDLL(ctypes.util.find_library("CoreGraphics"))
    core_graphics.CGPreflightScreenCaptureAccess.restype = ctypes.c_bool
    return bool(core_graphics.CGPreflightScreenCaptureAccess())


def clean_env():
    """The environment for Apple toolchain children.

    Apple's Python is built against a specific SDK and exports `SDKROOT` for
    it, which a `swiftc` child would inherit and then resolve against an SDK
    that need not match the selected Xcode. That mismatch is silent except for
    the compile getting slow enough to blow a timeout, so the variable is
    dropped rather than trusted.
    """
    env = dict(os.environ)
    env.pop("SDKROOT", None)
    return env


def build_window_list(source, dest):
    """Compile the window-list helper once, reusing it while its source is older.

    Interpreting the script with `swift` costs a compile per invocation and the
    caller polls: one `swiftc` now, milliseconds per poll after.
    """
    if dest.exists() and dest.stat().st_mtime >= source.stat().st_mtime:
        return dest
    compiled = subprocess.run(["swiftc", "-O", "-o", str(dest), str(source)],
                              capture_output=True, text=True, env=clean_env(), timeout=300)
    if compiled.returncode != 0:
        cannot_verify(f"swiftc could not build {source.name}:\n{compiled.stderr}")
    return dest


def app_pids(binary):
    """PIDs whose command line is this application, whatever launched it.

    `open` hands the process to launchd, so the caller holds no handle for the
    LaunchServices arms; the process table is the only common source.
    """
    listing = subprocess.run(["pgrep", "-f", str(binary)], capture_output=True, text=True)
    return {int(pid) for pid in listing.stdout.split()}


def windows_for_app(binary, window_list, timeout):
    """Poll the window list until this launch has a normal window on screen.

    Matched by owning PID, never by owner name. The name is what a human sees,
    but it is not unique to *this* launch: a window left behind by an earlier
    arm carries the same name, and accepting it would turn the gate into a
    tautology — a launch route that started nothing would be reported as
    rendering, on the strength of a window the previous arm opened. PIDs are
    exact for the process this call is watching.
    """
    deadline = time.monotonic() + timeout
    while True:
        pids = app_pids(binary)
        listing = subprocess.run([str(window_list)], capture_output=True,
                                 text=True, timeout=60)
        for line in listing.stdout.splitlines():
            fields = dict(part.split("=", 1) for part in line.split() if "=" in part)
            if fields.get("layer") == "0" and int(fields.get("pid", -1)) in pids:
                return int(fields["win"]), fields.get("size", "?"), fields.get("owner", "?")
        if time.monotonic() >= deadline:
            return None, None, None
        time.sleep(0.5)


def capture_window(number, png):
    subprocess.run(["screencapture", "-x", "-o", f"-l{number}", str(png)],
                   check=True, capture_output=True, timeout=60)
    if not png.exists():
        raise RuntimeError("screencapture produced no image")
    bmp = png.with_suffix(".bmp")
    subprocess.run(["sips", "-s", "format", "bmp", str(png), "--out", str(bmp)],
                   check=True, capture_output=True, timeout=60)
    return bmp


def content_histogram(bmp):
    """Bucket a window capture's content region, excluding title bar and shadow.

    The capture is the window *including* chrome, so a flat content area would
    otherwise be hidden behind the title bar's text and traffic lights. The
    insets are proportional: the title bar is a fixed ~32 points while the
    capture is in device pixels, so its share of the image shrinks on Retina.
    """
    data = bmp.read_bytes()
    offset = struct.unpack_from("<I", data, 10)[0]
    width = struct.unpack_from("<i", data, 18)[0]
    height = abs(struct.unpack_from("<i", data, 22)[0])
    bits = struct.unpack_from("<H", data, 28)[0]
    stride = ((width * bits // 8) + 3) // 4 * 4
    step = max(bits // 8, 1)
    counts = Counter()
    left, right = int(width * 0.06), int(width * 0.94)
    top, bottom = int(height * 0.13), int(height * 0.95)
    for y in range(top, bottom, 3):
        for x in range(left, right, 3):
            pixel = offset + y * stride + x * step
            blue, green, red = data[pixel], data[pixel + 1], data[pixel + 2]
            counts[(red // BUCKET * BUCKET, green // BUCKET * BUCKET, blue // BUCKET * BUCKET)] += 1
    return width, height, counts


def matches(observed, expected):
    return all(abs(channel - wanted) <= BUCKET for channel, wanted in zip(observed, expected))


def launch(arm, bundle, binary, log):
    """Start the application by one route, reporting why the route refused.

    `open` failing is a result, not an accident: LaunchServices rejecting the
    bundle is exactly the kind of launch-route defect this gate exists to see,
    and it has to arrive as a classified failure with `open`'s own message
    rather than as a traceback out of this script.
    """
    if arm == "direct":
        return subprocess.Popen([str(binary)], stdout=log, stderr=subprocess.STDOUT), None
    flags = ["open"] if arm == "launchservices" else ["open", "-g"]
    started = subprocess.run([*flags, str(bundle)], capture_output=True, text=True, timeout=60)
    if started.returncode != 0:
        detail = (started.stderr.strip() or started.stdout.strip() or "no message").splitlines()[0]
        return None, f"`{' '.join(flags)}` exited {started.returncode}: {detail}"
    return None, None


def stop(process, binary):
    """Terminate this launch and wait for it to leave the process table.

    Waiting is not politeness: the next arm searches the window list for *its*
    window, and a window this arm left behind would still be there to be found.
    The bound is generous because a LaunchServices app is asked to quit rather
    than killed outright.
    """
    if process is not None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
    # `open` hands the process to launchd, so there is no handle to wait on.
    subprocess.run(["pkill", "-f", str(binary)], capture_output=True, timeout=30)
    deadline = time.monotonic() + 15.0
    while time.monotonic() < deadline:
        if not app_pids(binary):
            time.sleep(1)
            return True
        time.sleep(0.5)
    return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=pathlib.Path,
                        help="an .app bundle, or a bare executable to stage into one")
    parser.add_argument("--runs", type=int, default=3, help="launches per arm")
    parser.add_argument("--expect", help="R,G,B the content must contain, e.g. 240,0,0")
    parser.add_argument("--output", type=pathlib.Path, default=ROOT / "target/macos-launch-render")
    options = parser.parse_args()
    if sys.platform != "darwin":
        cannot_verify("this check needs macOS with an active GUI session; a window that never "
                      "goes on screen cannot be photographed")
    if shutil.which("swiftc") is None:
        cannot_verify("`swiftc` is not on PATH, so the window list cannot be read")

    app = options.app.resolve()
    options.output.mkdir(parents=True, exist_ok=True)
    expected = tuple(int(part) for part in options.expect.split(",")) if options.expect else None
    if expected is not None and len(expected) != 3:
        # A short tuple would silently compare fewer channels than the caller
        # named, and `zip` would simply stop early — a wrong oracle that passes.
        parser.error("--expect takes three comma-separated channels, e.g. --expect 240,0,0")

    if not screen_capture_permitted():
        cannot_verify(
            "this host has not granted Screen Recording, so the window capture would come back "
            "blank whether or not the application rendered. Grant it to the terminal running "
            "this check (System Settings -> Privacy & Security -> Screen Recording) and re-run. "
            "A denied capture is not a blank window."
        )

    with tempfile.TemporaryDirectory(prefix="flui-launch-render-") as staging:
        window_list = build_window_list(WINDOW_LIST, options.output / "macos-window-list")
        if app.suffix == ".app":
            bundle = app
            with (bundle / "Contents/Info.plist").open("rb") as stream:
                plist = plistlib.load(stream)
            binary = bundle / "Contents/MacOS" / plist["CFBundleExecutable"]
        else:
            bundle = pathlib.Path(staging) / "LaunchRender.app"
            contents = bundle / "Contents"
            (contents / "MacOS").mkdir(parents=True)
            binary = contents / "MacOS" / app.name
            shutil.copy2(app, binary)
            with (contents / "Info.plist").open("wb") as stream:
                plistlib.dump({"CFBundlePackageType": "APPL",
                               "CFBundleName": app.name,
                               "CFBundleIdentifier": "dev.flui.launch-render-probe",
                               "CFBundleExecutable": app.name,
                               "NSPrincipalClass": "NSApplication"}, stream)
        print(f"bundle={bundle}\nruns={options.runs} expect={expected}")

        failures = []
        for arm in ARMS:
            for run in range(1, options.runs + 1):
                with (options.output / f"{arm}-{run}.log").open("wb") as log:
                    process, refused = launch(arm, bundle, binary, log)
                if refused is not None:
                    failures.append(f"{arm} run {run}: {refused}")
                    print(f"FAIL {arm} run {run}: {refused}")
                    stop(process, binary)
                    continue
                number, size, seen_owner = windows_for_app(binary, window_list, WINDOW_TIMEOUT)
                if number is None:
                    alive = len(app_pids(binary))
                    stop(process, binary)
                    failures.append(f"{arm} run {run}: no window within {WINDOW_TIMEOUT:g}s "
                                    f"({alive} matching process(es) alive)")
                    print(f"FAIL {arm} run {run}: no on-screen window appeared")
                    continue
                png = options.output / f"{arm}-{run}.png"
                try:
                    bmp = capture_window(number, png)
                    width, height, counts = content_histogram(bmp)
                finally:
                    exited = stop(process, binary)
                if not exited:
                    failures.append(f"{arm} run {run}: the application did not leave the process "
                                    f"table after being asked to quit")
                samples = sum(counts.values())
                buckets = len(counts)
                top = counts.most_common(1)[0]
                share = 100 * top[1] / samples
                print(f"  {arm} run {run}: window {number} owner={seen_owner} {size} "
                      f"captured {width}x{height} buckets={buckets} "
                      f"dominant={top[0]} ({share:.2f}%) ink={100 - share:.2f}%")
                reason = None
                if expected is not None:
                    matched = sum(count for colour, count in counts.items() if matches(colour, expected))
                    share_expected = 100 * matched / samples
                    print(f"    expected rgb{expected} present in {share_expected:.1f}% of samples")
                    if share_expected < MIN_EXPECT_SHARE:
                        reason = (f"content holds {share_expected:.2f}% of the expected colour "
                                  f"rgb{expected} (needs {MIN_EXPECT_SHARE:g}%) — the window drew "
                                  f"something other than the fixture")
                elif buckets < MIN_BUCKETS:
                    reason = (f"content region is flat ({buckets} colour bucket(s), "
                              f"{share:.1f}% one colour) — the window is blank")
                if reason is None:
                    print(f"PASS {arm} run {run}")
                else:
                    failures.append(f"{arm} run {run}: {reason}")
                    print(f"FAIL {arm} run {run}: {reason}")

    if failures:
        for failure in failures:
            print(f"FAILURE: {failure}", file=sys.stderr)
        print(f"MACOS_LAUNCH_RENDER=FAIL ({len(failures)} of {options.runs * len(ARMS)} launches; "
              f"images under {options.output})", file=sys.stderr)
        raise SystemExit(EXIT_BLANK)
    print(f"MACOS_LAUNCH_RENDER=PASS ({options.runs} launches on each of {len(ARMS)} routes; "
          f"images under {options.output})")
    raise SystemExit(EXIT_PASS)


if __name__ == "__main__":
    raise SystemExit(main())
