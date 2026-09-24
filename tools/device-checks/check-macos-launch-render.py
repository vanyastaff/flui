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
pump, and still show nothing — the iOS arm of `cargo xtask device ios-sim` records the same
lesson, where a UIKit layer repainted over the Metal layer while the app kept
logging frames. The capture is `screencapture -l <window number>`, taken by
window number rather than by screen region so an occluded or never-activated
window is still read from its own backing store; a full-screen grab would
photograph whatever is on top of it and turn the background arm into a
tautology.

The oracle is asked of every capture, not only of the first one. A macOS window
is ordered front before its first frame is presented — the first show happens
ahead of the GPU stack — so a cold launch has its window on screen for seconds
with nothing drawn in it, and a gate that captures once calls that startup a
rendering defect. Each launch is therefore given a bounded settle: the oracle is
retried until it holds, and the time from the window appearing to the capture
that passed is reported as `first frame after`. "Never drew" and "has not drawn
yet" stop being the same reading, and a window that is genuinely blank now fails
as one that stayed blank for the whole bound.

That retry has a converse worth stating, because it is easy to misread a
failing capture. `screencapture -l` on an ordered-front window that has no
backing store yet returns a flat dark image whatever the display shows, so the
window-scoped capture cannot say what a person had on screen during the gap.
When a launch fails, this check therefore takes one *screen* capture of the
window's rectangle as well — only when that window is the frontmost one, so the
pixels belong to it — and reports it as what a viewer saw. It decides nothing:
the window-scoped capture stays the oracle, because a region capture photographs
whatever is on top of the rectangle.

That capture is the one part of this gate the host can revoke: reading another
application's window needs Screen Recording. The permission is preflighted
(`CGPreflightScreenCaptureAccess`) and its absence is reported as
CANNOT VERIFY, never as a blank window — without that distinction a denied
capture and the defect look identical.

The window number comes from `tools/device-checks/macos-window-list.swift`; see its header
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

ROOT = pathlib.Path(__file__).resolve().parents[2]
WINDOW_LIST = ROOT / "tools/device-checks/macos-window-list.swift"

# Arms, in the order they are reported. `direct` is the control: if it fails
# too, the launch route is not what broke.
ARMS = ("direct", "launchservices", "launchservices-background")

# How long a launched application gets to put a window on screen.
WINDOW_TIMEOUT = 20.0

# How long a window that has appeared gets to hold content before it is called
# blank, and how long to wait between attempts. The retry exists because the
# window is ordered front ahead of its first frame: a cold launch has it on
# screen, unpainted, for seconds. The bound is generous next to that measured
# 2.81 s so the gate decides "never drew" rather than "was slow once", and an
# application that renders normally passes on its first attempt and pays
# nothing. The interval is small because each attempt already costs a
# `screencapture` and a `sips`.
SETTLE_TIMEOUT = 10.0
SETTLE_INTERVAL = 0.25

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


def window_listing(window_list):
    """Every on-screen window, front to back, as parsed `key=value` fields.

    The helper prints in `CGWindowListCopyWindowInfo` order, which is front to
    back, and that order is load-bearing: it is how the frontmost window is
    identified without asking the window server for anything more.
    """
    listing = subprocess.run([str(window_list)], capture_output=True, text=True, timeout=60)
    rows = []
    for line in listing.stdout.splitlines():
        fields = dict(part.split("=", 1) for part in line.split() if "=" in part)
        if fields:
            rows.append(fields)
    return rows


def windows_for_app(binary, window_list, timeout):
    """Poll the window list until this launch has a normal window on screen.

    Matched by owning PID, never by owner name. The name is what a human sees,
    but it is not unique to *this* launch: a window left behind by an earlier
    arm carries the same name, and accepting it would turn the gate into a
    tautology — a launch route that started nothing would be reported as
    rendering, on the strength of a window the previous arm opened. PIDs are
    exact for the process this call is watching.

    The sighting carries the moment the window was first seen, because that and
    not the launch is the honest start of the "it has not drawn yet" interval:
    an application is free to take its time before there is any window at all,
    and that delay is not a blank window.
    """
    deadline = time.monotonic() + timeout
    while True:
        pids = app_pids(binary)
        for fields in window_listing(window_list):
            if fields.get("layer") == "0" and int(fields.get("pid", -1)) in pids:
                return {"number": int(fields["win"]),
                        "size": fields.get("size", "?"),
                        "origin": fields.get("origin", "?"),
                        "owner": fields.get("owner", "?"),
                        "appeared": time.monotonic()}
        if time.monotonic() >= deadline:
            return None
        time.sleep(0.5)


def frontmost_window(window_list):
    """The number of the frontmost normal window, or None if there is none."""
    for fields in window_listing(window_list):
        if fields.get("layer") == "0":
            return int(fields["win"])
    return None


def viewer_rect(sighting):
    """The window's on-screen rectangle, in the form `screencapture -R` takes.

    `CGWindowListCopyWindowInfo` reports bounds in the global display space and
    `-R` reads that same space, so the numbers pass through unchanged — both are
    points on the display, not pixels of a capture.

    A sighting with no bounds yet yields no rectangle. A region built from a
    missing bound would photograph a corner of the desktop and be filed as the
    window's appearance, which is worse than reporting that nothing was taken.
    """
    size, origin = sighting.get("size", "?"), sighting.get("origin", "?")
    if "x" not in size or "," not in origin:
        return None
    width, height = (int(part) for part in size.split("x"))
    left, top = (int(part) for part in origin.split(","))
    return f"{left},{top},{width},{height}"


def capture_window(number, png):
    subprocess.run(["screencapture", "-x", "-o", f"-l{number}", str(png)],
                   check=True, capture_output=True, timeout=60)
    if not png.exists():
        raise RuntimeError("screencapture produced no image")
    bmp = png.with_suffix(".bmp")
    subprocess.run(["sips", "-s", "format", "bmp", str(png), "--out", str(bmp)],
                   check=True, capture_output=True, timeout=60)
    return bmp


def capture_viewer(region, bmp):
    """Photograph the pixels a viewer sees inside the window's rectangle.

    Deliberately not the oracle. The window-scoped capture reads the window's
    own backing store, which is what lets an occluded or never-activated window
    be measured at all — the background arm depends on it. Its converse is that
    an ordered-front window with no backing store reports a flat dark image
    whatever the display shows, so the one thing it cannot say is what a person
    had on screen. This capture says only that, and decides nothing: it
    photographs whatever is on top of the rectangle.
    """
    subprocess.run(["screencapture", "-x", "-R", region, "-t", "bmp", str(bmp)],
                   check=True, capture_output=True, timeout=60)
    if not bmp.exists():
        raise RuntimeError("screencapture produced no image")
    return bmp


def content_histogram(bmp):
    """Bucket a capture's content region, excluding title bar and shadow.

    The capture is the window *including* chrome, so a flat content area would
    otherwise be hidden behind the title bar's text and traffic lights. The
    insets are proportional: the title bar is a fixed ~32 points while the
    capture is in device pixels, so its share of the image shrinks on Retina.

    The same insets are correct for both instruments, because both frame the
    window the same way: `-l` captures the window with its chrome, and the
    region capture is handed exactly the window's rectangle with no shadow.
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


def judge(counts, expected):
    """Why this capture is not a rendered window, or None when it is one.

    The whole oracle lives here because it is now asked of every attempt in the
    settle loop rather than once: the loop has to decide, at each attempt, the
    same question the final verdict asks, and two copies of it could drift.
    """
    samples = sum(counts.values())
    if expected is not None:
        matched = sum(count for colour, count in counts.items() if matches(colour, expected))
        share = 100 * matched / samples
        if share < MIN_EXPECT_SHARE:
            return (f"content holds {share:.2f}% of the expected colour rgb{expected} "
                    f"(needs {MIN_EXPECT_SHARE:g}%) — the window drew something other than the "
                    f"fixture")
        return None
    buckets = len(counts)
    if buckets < MIN_BUCKETS:
        share = 100 * counts.most_common(1)[0][1] / samples
        return (f"content region is flat ({buckets} colour bucket(s), {share:.1f}% one colour) "
                f"— the window is blank")
    return None


def viewer_report(sighting, window_list, scratch):
    """One sentence on what a viewer had on screen in the window's rectangle.

    Taken only when this window is the frontmost normal window. A region capture
    reads whatever is on top of the rectangle, so with the window behind
    something — the normal case for the background launch arm, which never
    activates — the pixels would belong to that other window and the sentence
    would describe it. Saying nothing was taken is the honest answer there, and
    it keeps this diagnostic from ever contradicting the oracle.
    """
    frontmost = frontmost_window(window_list)
    if frontmost != sighting["number"]:
        return (f"A screen capture of the window's rectangle was not taken: window "
                f"{sighting['number']} is not the frontmost window (that is {frontmost}), so the "
                f"region would hold whatever covers it.")
    region = viewer_rect(sighting)
    if region is None:
        return "A screen capture of the window's rectangle was not taken: no bounds were reported."
    try:
        bmp = capture_viewer(region, scratch)
    except (subprocess.CalledProcessError, RuntimeError) as error:
        return f"A screen capture of the window's rectangle could not be taken: {error}"
    _, _, counts = content_histogram(bmp)
    samples = sum(counts.values())
    top = counts.most_common(1)[0]
    return (f"A screen capture of the same rectangle shows rgb{top[0]} over "
            f"{100 * top[1] / samples:.2f}% of it ({len(counts)} bucket(s)) — what a viewer had on "
            f"screen, which the window-scoped capture above cannot show for a window with no "
            f"backing store.")


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
                sighting = windows_for_app(binary, window_list, WINDOW_TIMEOUT)
                if sighting is None:
                    alive = len(app_pids(binary))
                    stop(process, binary)
                    failures.append(f"{arm} run {run}: no window within {WINDOW_TIMEOUT:g}s "
                                    f"({alive} matching process(es) alive)")
                    print(f"FAIL {arm} run {run}: no on-screen window appeared")
                    continue
                number = sighting["number"]
                png = options.output / f"{arm}-{run}.png"
                drawn_at = None
                try:
                    # The window goes on screen ahead of its first frame, so the
                    # first capture of a cold launch is of a window with nothing
                    # drawn in it. Retry the oracle instead of deciding on one
                    # look: "never drew" and "has not drawn yet" are the same
                    # reading, and only a second attempt tells them apart.
                    deadline = sighting["appeared"] + SETTLE_TIMEOUT
                    attempt = 0
                    while True:
                        attempt += 1
                        bmp = capture_window(number, png)
                        width, height, counts = content_histogram(bmp)
                        reason = judge(counts, expected)
                        if reason is None:
                            drawn_at = time.monotonic()
                            break
                        if attempt == 1:
                            # The one artefact worth keeping out of the gap, and
                            # one to read carefully: it is a window-scoped
                            # capture, so on an unpainted window it is the
                            # flat-dark capture artifact rather than the
                            # near-white screen — see `capture_viewer`.
                            shutil.copy2(png, options.output / f"{arm}-{run}-undrawn.png")
                        if time.monotonic() >= deadline:
                            break
                        print(f"    attempt {attempt} at "
                              f"{time.monotonic() - sighting['appeared']:.2f}s: {reason}; retrying")
                        time.sleep(SETTLE_INTERVAL)
                finally:
                    exited = stop(process, binary)
                if not exited:
                    failures.append(f"{arm} run {run}: the application did not leave the process "
                                    f"table after being asked to quit")
                samples = sum(counts.values())
                buckets = len(counts)
                top = counts.most_common(1)[0]
                share = 100 * top[1] / samples
                print(f"  {arm} run {run}: window {number} owner={sighting['owner']} "
                      f"{sighting['size']} captured {width}x{height} buckets={buckets} "
                      f"dominant={top[0]} ({share:.2f}%) ink={100 - share:.2f}%")
                if drawn_at is None:
                    print(f"    first frame: not within {SETTLE_TIMEOUT:g}s of the window "
                          f"appearing ({attempt} capture(s))")
                else:
                    print(f"    first frame after {drawn_at - sighting['appeared']:.2f}s "
                          f"({attempt} capture(s))")
                reason = judge(counts, expected)
                if expected is not None:
                    # Printed from the capture that decided, because the number
                    # the oracle compared against is the one worth showing even
                    # when it passed.
                    matched = sum(count for colour, count in counts.items() if matches(colour, expected))
                    print(f"    expected rgb{expected} present in "
                          f"{100 * matched / samples:.1f}% of samples")
                if reason is None:
                    print(f"PASS {arm} run {run}")
                else:
                    if drawn_at is None:
                        reason += (f", and it looked the same on every capture for {SETTLE_TIMEOUT:g}s "
                                   f"after the window appeared — this is a window that never drew, "
                                   f"not one caught before its first frame")
                    viewer_bmp = options.output / f"{arm}-{run}-viewer.bmp"
                    print(f"    {viewer_report(sighting, window_list, viewer_bmp)}")
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
