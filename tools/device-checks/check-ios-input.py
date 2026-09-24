#!/usr/bin/env python3
"""Verify that a real touch reaches a flui application on iOS, and that what it
changed survives a real background/foreground round trip.

The recorded gap this closes (`docs/BETA.md` § "iOS execution lifecycle
foundation") is that the simulator UI automation timed out, so "real touch input
and retained displayed counter state after Home/return" were unverified. Two
things were missing: an instrument that can put a `UITouch` into the
application, and an oracle that can tell whether anything reached the display.

The instrument is XCUITest. `xcrun simctl` has no touch subcommand, and driving
the Simulator window through host UI automation needs the Accessibility grant —
it also photographs the host's desktop rather than the device. XCUITest
synthesises touches inside the simulator through the platform's own automation
channel: the touch is real, and the host needs no desktop permission. Nothing is
built here that ships; a test bundle is compiled against the simulator SDK's
XCTest and run against an already-staged application.

The oracle is the screen, and it has to be. `flui-platform` publishes no
accessibility tree on iOS — its `a11y` bridge covers AT-SPI, UIA and
NSAccessibility, not UIKit — so a UI test cannot read a widget by identifier.
What it can do is photograph the screen and compare, which is the same discipline
`cargo xtask device macos-launch-render` uses, and for the same reason: an application can
report state it never drew.

Comparison is exact, over the whole screen minus the status bar and the home
indicator, because the clock ticks on its own. Five stages are measured in one
launch by `tools/device-checks/ios-input-probe.swift`, and two of them are controls, so a
pass is a discrimination rather than an absence of measurement:

  tap            a real touch on a list row changes the displayed selection
  resume         Home, then return, and the selection is still displayed
  resume-tap     optional (`--post-return-tap`): a touch after the return
                 advances the display again, so the resumed screen is a live
                 application and not the system's snapshot of the pre-Home
                 frame. Equality across `resume` cannot tell the two apart,
                 because that snapshot is the pre-Home frame by construction
  relaunch       a fresh launch shows the initial selection again, so `resume`
                 could have failed instead of being trivially true
  no-tap         a real touch that lands on no target changes nothing, so `tap`
                 distinguishes a hit from any touch

Both tap points must lie inside the compared region, and that is enforced
rather than documented. A no-target control measured outside the window it is a
control for cannot fail — a touch that did change the display there would still
be reported as an unchanged one — so its pass would say nothing. The default
point is the demo's app bar centre for exactly this reason: the app bar is the
only target-free area of that screen, and the title baseline it used to be was
cropped out by the region's status-bar inset.

Exit codes: 0 all stages held, 1 a stage failed (the framework's own behaviour),
2 the host could not take the measurement (no Xcode toolchain, no booted
simulator, the probe produced no report, evidence could not be read back).
Every toolchain command this script runs is wrapped, and a tool that fails to
answer is 2 rather than an uncaught traceback: the gap this gate closes was
itself an automation *timeout*, which is exactly the host fault most likely to
recur. Two is a loud non-pass, never a quiet one: a harness that never ran must
not be filed as a defect in the framework, and a defect in the framework must
not be filed as a harness problem and retried. `argparse` also exits 2 on a
rejected argument, so a malformed `--region` reaches the caller through the
same channel; it is a run that did not happen, which is what 2 means.
"""
import argparse
import json
import os
import pathlib
import plistlib
import shutil
import subprocess
import sys
import tempfile
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[2]
PROBE_SOURCE = ROOT / "tools/device-checks/ios-input-probe.swift"

# The test runner installed on the simulator. It is Apple's XCTRunner, patched
# with the identity the toolchain normally substitutes at build time.
RUNNER_IDENTIFIER = "dev.flui.ios-input-runner"
RUNNER_NAME = "ProbeRunner"

# Frameworks the runner and the test bundle need at runtime. XCTRunner links
# `@rpath/XCTest.framework/XCTest` and its rpath is `@executable_path/Frameworks`,
# so embedding is what makes `xcodebuild test-without-building` work on an
# artifact no Xcode project produced. XCTest's own dependencies live in the
# platform's PrivateFrameworks, and libXCTestSwiftSupport carries the Swift
# overlay the assertions are implemented in.
FRAMEWORKS = ("XCTest", "XCUIAutomation", "Testing", "_Testing_Foundation")
PRIVATE_FRAMEWORKS = ("XCTestCore", "XCTestSupport", "XCTAutomationSupport")
SWIFT_SUPPORT = "libXCTestSwiftSupport.dylib"

EXIT_PASS = 0
EXIT_FAIL = 1
EXIT_CANNOT_VERIFY = 2


def cannot_verify(reason):
    """Report a measurement that could not be taken, distinctly from a failure."""
    print(f"IOS_INPUT=CANNOT_VERIFY: {reason}", file=sys.stderr)
    raise SystemExit(EXIT_CANNOT_VERIFY)


def command(*args, timeout=120, env=None):
    """Read one fact from the Apple toolchain, or report that we could not.

    Every use of this helper is a question the measurement cannot proceed
    without, so a toolchain that fails to answer is CANNOT VERIFY — never FAIL.
    That is the entire point of the third exit code: a missing Xcode, a down
    CoreSimulator service, a sandbox denial or a timeout are all statements
    about the host, and filing any of them as a defect in the framework would
    send someone to fix flui for a machine's problem. The recorded gap this gate
    closes was itself a simulator automation *timeout*, which is exactly the
    host fault most likely to recur — so it must not arrive as a touch failure.
    """
    try:
        return subprocess.run(args, check=True, text=True, capture_output=True,
                              timeout=timeout, env=env).stdout.strip()
    except FileNotFoundError as error:
        cannot_verify(f"{args[0]} is not on PATH ({error})")
    except subprocess.TimeoutExpired:
        cannot_verify(f"{' '.join(args)} did not answer within {timeout}s")
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "").strip()
        cannot_verify(f"{' '.join(args)} failed (exit {error.returncode}): {detail}")


def parse_region(value):
    """Parse `left,top,right,bottom` as four fractions, or refuse to run.

    Validated here rather than left to the probe, because a probe that receives
    a malformed region can only either refuse to run at all or substitute a
    default — and a substituted default has already produced a confident wrong
    answer once: the counter's digit sits under the status-bar clock, and the
    default region's 6% top inset crops out exactly the pixels its tap changes,
    which makes a delivered touch and a lost one look identical. A typo in the
    geometry must therefore be an error, never a default, and the value printed
    into the run's header must be the value measured.
    """
    parts = value.split(",")
    if len(parts) != 4:
        raise argparse.ArgumentTypeError(f"expected 'left,top,right,bottom', got {value!r}")
    try:
        left, top, right, bottom = (float(part) for part in parts)
    except ValueError:
        raise argparse.ArgumentTypeError(f"the four fractions must be numbers, got {value!r}")
    if not (0.0 <= left < right <= 1.0 and 0.0 <= top < bottom <= 1.0):
        raise argparse.ArgumentTypeError(
            f"the region must satisfy 0 <= left < right <= 1 and 0 <= top < bottom <= 1, "
            f"got {value!r}")
    return value


def region_of(value):
    """The four fractions of an already-validated region."""
    left, top, right, bottom = (float(part) for part in value.split(","))
    return left, top, right, bottom


def point_of(value):
    """The two coordinates of an already-validated tap."""
    dx, dy = (float(part) for part in value.split(","))
    return dx, dy


def parse_tap(value):
    """Parse `dx,dy` as two normalized coordinates, or refuse to run."""
    parts = value.split(",")
    if len(parts) != 2:
        raise argparse.ArgumentTypeError(f"expected 'dx,dy', got {value!r}")
    try:
        dx, dy = (float(part) for part in parts)
    except ValueError:
        raise argparse.ArgumentTypeError(f"the two coordinates must be numbers, got {value!r}")
    if not (0.0 <= dx <= 1.0 and 0.0 <= dy <= 1.0):
        raise argparse.ArgumentTypeError(
            f"the tap must land inside the screen (0 <= dx, dy <= 1), got {value!r}")
    return value


def clean_env():
    """The environment for Apple toolchain children.

    Apple's Python is built against a specific SDK and exports `SDKROOT` for it,
    which a `swiftc` child would inherit and resolve against an SDK that need
    not match the selected Xcode.
    """
    env = dict(os.environ)
    env.pop("SDKROOT", None)
    return env


def developer_directory():
    # `command` already turns a missing or broken xcode-select into CANNOT
    # VERIFY, so there is nothing to catch here.
    return pathlib.Path(command("xcode-select", "-p", env=clean_env()))


def simulator_target(developer):
    """The triple the test bundle and the runner are compiled for, and the
    platform Developer directory holding the XCTest frameworks they link."""
    sdk = command("xcrun", "--sdk", "iphonesimulator", "--show-sdk-path", env=clean_env())
    version = command("xcrun", "--sdk", "iphonesimulator", "--show-sdk-version", env=clean_env())
    platform = developer / "Platforms/iPhoneSimulator.platform/Developer"
    return sdk, f"arm64-apple-ios{version}-simulator", platform


def require_booted(udid):
    """The selected simulator must exist, be available, and already be booted.

    Booting one here would be this script deciding which device the measurement
    describes; the caller names it, as the other iOS checks do.
    """
    inventory = json.loads(command("xcrun", "simctl", "list", "devices", "--json"))
    selected = [device for devices in inventory["devices"].values() for device in devices
                if device["udid"] == udid and device.get("isAvailable")]
    if len(selected) != 1:
        cannot_verify(f"{udid} is not one available simulator")
    if selected[0]["state"] != "Booted":
        cannot_verify(f"{selected[0]['name']} ({udid}) is {selected[0]['state']}, not Booted")


def copy(source, destination):
    """Copy a toolchain artifact, or report that this toolchain lacks it.

    A missing XCTRunner or XCTest framework means this Xcode cannot host a UI
    test at all. That is a fact about the host, so it is CANNOT VERIFY — the
    same rule `command` applies to a toolchain that will not answer.
    """
    try:
        if source.is_dir():
            shutil.copytree(source, destination)
        else:
            shutil.copy2(source, destination)
    except OSError as error:
        cannot_verify(f"{source} could not be staged into the runner ({error})")


def build_runner(developer, out, target, sdk):
    """Assemble the test runner xcodebuild would have built.

    XCTRunner.app ships with three unsubstituted placeholders in its Info.plist
    — the toolchain fills them for a generated runner, and xcodebuild reads the
    values literally. An unsubstituted CFBundleExecutable is reported as
    "missing its bundle executable", so the placeholders are replaced here and
    then re-checked, because a future Xcode adding a fourth would otherwise
    surface as that same misleading install error.
    """
    runner = out / f"{RUNNER_NAME}.app"
    copy(developer / "Library/Xcode/Agents/XCTRunner.app", runner)
    plist = runner / "Info.plist"
    for key, value in (("CFBundleExecutable", "XCTRunner"),
                       ("CFBundleIdentifier", RUNNER_IDENTIFIER),
                       ("CFBundleName", RUNNER_NAME)):
        command("plutil", "-replace", key, "-string", value, str(plist))
    leftover = [line for line in command("plutil", "-p", str(plist)).splitlines() if "$(" in line]
    if leftover:
        cannot_verify("the runner's Info.plist still holds unsubstituted placeholders: "
                      + "; ".join(leftover))

    frameworks = runner / "Frameworks"
    frameworks.mkdir()
    for name in FRAMEWORKS:
        copy(developer / f"Library/Frameworks/{name}.framework", frameworks / f"{name}.framework")
    for name in PRIVATE_FRAMEWORKS:
        copy(developer / f"Library/PrivateFrameworks/{name}.framework",
             frameworks / f"{name}.framework")
    copy(developer / f"usr/lib/{SWIFT_SUPPORT}", frameworks / SWIFT_SUPPORT)
    return runner


def build_test_bundle(developer, out, target, sdk):
    """Compile the probe into the `.xctest` bundle xcodebuild will install.

    A test bundle is a loadable bundle, not an executable: it is compiled with
    `-emit-library` and its own Info.plist, and it finds XCTest through the
    runner's embedded Frameworks directory.
    """
    bundle = out / "FluiIOSInputProbe.xctest"
    bundle.mkdir()
    compiled = subprocess.run(
        ["swiftc", "-sdk", sdk, "-target", target, "-parse-as-library", "-emit-library",
         "-F", str(developer / "Library/Frameworks"), "-I", str(developer / "usr/lib"),
         "-L", str(developer / "usr/lib"), "-framework", "XCTest", "-framework", "XCUIAutomation",
         "-Xlinker", "-rpath", "-Xlinker", "@executable_path/Frameworks",
         "-o", str(bundle / "FluiIOSInputProbe"), str(PROBE_SOURCE)],
        capture_output=True, text=True, env=clean_env(), timeout=600)
    if compiled.returncode != 0:
        cannot_verify(f"swiftc could not build the test bundle:\n{compiled.stderr}")
    with (bundle / "Info.plist").open("wb") as stream:
        plistlib.dump({"CFBundleIdentifier": "dev.flui.ios-input-probe",
                       "CFBundleExecutable": "FluiIOSInputProbe",
                       "CFBundleName": "FluiIOSInputProbe",
                       "CFBundlePackageType": "BNDL",
                       "CFBundleVersion": "1",
                       "CFBundleShortVersionString": "1.0",
                       "MinimumOSVersion": "14.0",
                       "CFBundleSupportedPlatforms": ["iPhoneSimulator"]}, stream)
    return bundle


def write_test_run(out, app, bundle, runner, bundle_identifier, run_identifier, taps, region,
                   post_return_tap):
    """Describe the run to xcodebuild.

    `TestHostPath` is the runner (for UI tests, the runner *is* the host) and
    `UITargetAppPath` is the application under test. The bundle identifier
    reaches the probe through the test host's environment, so the same compiled
    bundle can be pointed at a different application without rebuilding, and the
    run identifier gives the probe a directory no earlier run can have written
    to. The tap points, the compared region and the choice of resume oracle
    travel the same way, because all of them are geometry and policy that belong
    to the application under test.
    """
    plan = {
        "TestPlan": {"Name": "flui-ios-input", "IsDefault": True},
        "TestConfigurations": [{
            "Name": "Configuration 1",
            "IsEnabled": True,
            "TestTargets": [{
                "BlueprintName": "FluiIOSInputProbe",
                "TestBundlePath": f"__TESTROOT__/{bundle.name}",
                "TestHostPath": f"__TESTROOT__/{runner.name}",
                "UITargetAppPath": f"__TESTROOT__/{app.name}",
                "TestHostBundleIdentifier": RUNNER_IDENTIFIER,
                "DependentProductPaths": [f"__TESTROOT__/{app.name}", f"__TESTROOT__/{bundle.name}"],
                "IsUITestBundle": True,
                "ProductModuleName": "FluiIOSInputProbe",
                "EnvironmentVariables": {"FLUI_IOS_PROBE_BUNDLE": bundle_identifier,
                                         "FLUI_IOS_PROBE_RUN": run_identifier,
                                         "FLUI_IOS_PROBE_TARGET_TAP": taps[0],
                                         "FLUI_IOS_PROBE_EMPTY_TAP": taps[1],
                                         "FLUI_IOS_PROBE_REGION": region,
                                         "FLUI_IOS_PROBE_POST_RETURN_TAP":
                                             "1" if post_return_tap else "0"},
            }],
        }],
        "__xctestrun_metadata__": {"FormatVersion": 2},
    }
    path = out / "flui-ios-input.xctestrun"
    with path.open("wb") as stream:
        plistlib.dump(plan, stream)
    return path


def container_of(udid, identifier):
    """The simulator container for an installed application, or None.

    The probe reports into its own container, which is how its evidence leaves
    the simulator: XCUITest's verdicts live in a result bundle, but a result
    bundle also holds a full build log for every stage, and reading one integer
    out of it is a lot of parsing for a file the probe can write directly.
    """
    listing = subprocess.run(["xcrun", "simctl", "get_app_container", udid, identifier, "data"],
                             capture_output=True, text=True, timeout=60)
    return pathlib.Path(listing.stdout.strip()) if listing.returncode == 0 else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("udid", help="an already booted simulator")
    parser.add_argument("app", type=pathlib.Path, help="the staged .app to drive")
    parser.add_argument("--output", type=pathlib.Path, default=ROOT / "target/ios-input")
    parser.add_argument("--target-tap", default="0.5,0.138", type=parse_tap,
                        help="normalized 'dx,dy' that must hit a widget "
                             "(default: the Material demo's first list row)")
    parser.add_argument("--empty-tap", default="0.5,0.07", type=parse_tap,
                        help="normalized 'dx,dy' that must hit nothing. It must also lie "
                             "inside --region, because a control measured outside the window "
                             "it is a control for cannot fail — that is an invariant this "
                             "script enforces, not a convention. On the demo's home screen "
                             "only the app bar has no target, so this point is its centre "
                             "rather than the title baseline it used to be, which the default "
                             "region's status-bar inset had cropped out (default: the Material "
                             "demo's app bar centre)")
    parser.add_argument("--region", default="0.0,0.06,1.0,0.94", type=parse_region,
                        help="compared region as normalized 'left,top,right,bottom'. It MUST "
                             "contain the pixels the target tap is meant to change: a region "
                             "that crops them out makes a delivered touch and a lost one look "
                             "identical, and the run then reports a lost touch that was never "
                             "lost (default: full width, status bar and home indicator excluded)")
    parser.add_argument("--post-return-tap", action="store_true",
                        help="prove the resumed screen is a live application and not the "
                             "system's snapshot of the pre-Home frame, by tapping the target "
                             "again after Home/return and requiring the display to advance. "
                             "Without it, retention rests on display equality alone, which a "
                             "snapshot satisfies by construction (see the probe's oracle). "
                             "Only usable when a second tap on the target changes the display "
                             "again — an idempotent subject (an already-selected row) cannot "
                             "use it, and the run's report names which oracle carried the claim")
    options = parser.parse_args()

    # Every tap has to be measured by the region it is meant to affect, or the
    # comparison is not about that tap at all. The no-target control is where
    # this failed silently: it tapped above the region's top edge, so a touch
    # that did change the display there would still have been reported as an
    # unchanged one, and the control could only ever pass.
    left, top, right, bottom = region_of(options.region)
    for option, (dx, dy) in (("--target-tap", point_of(options.target_tap)),
                             ("--empty-tap", point_of(options.empty_tap))):
        if not left <= dx <= right or not top <= dy <= bottom:
            parser.error(f"{option} {dx},{dy} lies outside the compared region "
                         f"{left},{top},{right},{bottom}, so no tap there can be seen; "
                         f"widen --region or move the tap into it")

    if sys.platform != "darwin":
        cannot_verify("this check needs macOS with Xcode and the iOS Simulator")
    if shutil.which("swiftc") is None:
        cannot_verify("`swiftc` is not on PATH, so the probe cannot be built")
    require_booted(options.udid)

    app = options.app.resolve()
    if not app.is_dir() or app.suffix != ".app":
        parser.error("the application argument must be a staged .app directory")
    with (app / "Info.plist").open("rb") as stream:
        manifest = plistlib.load(stream)
    bundle_identifier = manifest["CFBundleIdentifier"]

    # Names this run's evidence, so nothing a previous run left behind can be
    # read as this one's (see the container clearing below).
    run_identifier = uuid.uuid4().hex[:12]

    options.output.mkdir(parents=True, exist_ok=True)
    result_bundle = options.output / "result.xcresult"
    if result_bundle.exists():
        shutil.rmtree(result_bundle)
    # The evidence from this run goes to its own directory under the output
    # too, so a file copied out of the container can never be mistaken for an
    # earlier run's.
    evidence = options.output / run_identifier
    if evidence.exists():
        shutil.rmtree(evidence)
    evidence.mkdir()

    # The runner's container survives reinstallation, so anything a previous
    # run left in the probe's tmp/ is cleared first: a stale screenshot of a
    # stage that never happened is indistinguishable from a real one, and
    # reading one as this run's evidence would let a failed run pass.
    container = container_of(options.udid, RUNNER_IDENTIFIER)
    if container is not None:
        shutil.rmtree(container / "tmp/flui-ios-input", ignore_errors=True)

    developer = developer_directory()
    sdk, target, platform = simulator_target(developer)
    print(f"device={options.udid}\napp={app.name} bundle={bundle_identifier}\n"
          f"targetTap={options.target_tap} emptyTap={options.empty_tap} region={options.region}\n"
          f"resumeOracle={'display+live-touch' if options.post_return_tap else 'display-equality'}\n"
          f"output={options.output}")

    with tempfile.TemporaryDirectory(prefix="flui-ios-input-") as staging:
        staged = pathlib.Path(staging) / app.name
        copy(app, staged)
        runner = build_runner(platform, pathlib.Path(staging), target, sdk)
        bundle = build_test_bundle(platform, pathlib.Path(staging), target, sdk)
        plan = write_test_run(pathlib.Path(staging), staged, bundle, runner,
                              bundle_identifier, run_identifier,
                              (options.target_tap, options.empty_tap), options.region,
                              options.post_return_tap)
        try:
            executed = subprocess.run(
                ["xcodebuild", "test-without-building", "-xctestrun", str(plan),
                 "-destination", f"platform=iOS Simulator,id={options.udid}",
                 "-resultBundlePath", str(result_bundle)],
                capture_output=True, text=True, env=clean_env(), timeout=1800,
                cwd=staging)
        except FileNotFoundError:
            cannot_verify("xcodebuild is not on PATH, so the probe cannot be run")
        except subprocess.TimeoutExpired:
            # The gap this gate closes was recorded as a simulator automation
            # timeout. A timeout is the host failing to take the measurement,
            # so it must arrive as CANNOT VERIFY rather than as a lost touch —
            # otherwise the likeliest recurrence of the original problem is
            # filed as a defect in the framework and someone goes looking for a
            # hit-test bug that does not exist.
            cannot_verify("xcodebuild did not finish within 1800s, so no stage was measured")
    with (options.output / "xcodebuild.log").open("w") as log:
        log.write(executed.stdout + executed.stderr)

    # The run installs the runner, so its container is read after the fact.
    # A failure to read the report back is the host, not the framework: the
    # container path can be unavailable, and the report is the only channel
    # the verdict travels through.
    text = None
    try:
        located = container_of(options.udid, RUNNER_IDENTIFIER)
        if located is not None:
            written = located / "tmp/flui-ios-input" / run_identifier
            found = written / "report.txt"
            if found.exists():
                text = found.read_text()
                for artifact in written.iterdir():
                    shutil.copy2(artifact, evidence / artifact.name)
    except OSError as error:
        cannot_verify(f"the probe's report could not be read back from the simulator "
                      f"container ({error})")
    if text is None:
        tail = "\n".join((executed.stdout + executed.stderr).splitlines()[-25:])
        cannot_verify("the probe produced no report; xcodebuild said:\n" + tail
                      + f"\n(full log: {options.output / 'xcodebuild.log'})")

    print(text, end="")
    # Every exit carries the evidence directory: a failure is read from the
    # screenshots, and a CANNOT VERIFY has to be diagnosable without a rerun.
    verdict = text.strip().splitlines()[-1] if text.strip() else ""
    if verdict.startswith("PASS "):
        print(f"IOS_INPUT=PASS (measurement under {evidence})")
        raise SystemExit(EXIT_PASS)
    if verdict.startswith("CANNOT_VERIFY "):
        cannot_verify(verdict.removeprefix("CANNOT_VERIFY ") + f" (evidence under {evidence})")
    if verdict.startswith("FAIL "):
        print(f"IOS_INPUT=FAIL: {verdict.removeprefix('FAIL ')} (evidence under {evidence})",
              file=sys.stderr)
        raise SystemExit(EXIT_FAIL)
    cannot_verify(f"the probe's last line was not a verdict: {verdict!r}")


if __name__ == "__main__":
    raise SystemExit(main())
