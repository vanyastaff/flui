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
`just macos-launch-render` uses, and for the same reason: an application can
report state it never drew.

Comparison is exact, over the whole screen minus the status bar and the home
indicator, because the clock ticks on its own. Four stages are measured in one
launch by `scripts/ios-input-probe.swift`, and two of them are controls, so a
pass is a discrimination rather than an absence of measurement:

  tap        a real touch on a list row changes the displayed selection
  resume     Home, then return, and the selection is still displayed
  relaunch   a fresh launch shows the initial selection again, so `resume`
             could have failed instead of being trivially true
  no-tap     a real touch that lands on no target changes nothing, so `tap`
             distinguishes a hit from any touch

Exit codes: 0 all four stages held, 1 a stage failed (the framework's own
behaviour), 2 the host could not take the measurement (no Xcode toolchain, no
booted simulator, the probe produced no report). Two is a loud non-pass, never a
quiet one: a harness that never ran must not be filed as a defect in the
framework, and a defect in the framework must not be filed as a harness problem
and retried.
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

ROOT = pathlib.Path(__file__).resolve().parents[1]
PROBE_SOURCE = ROOT / "scripts/ios-input-probe.swift"

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
    return subprocess.run(args, check=True, text=True, capture_output=True,
                          timeout=timeout, env=env).stdout.strip()


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
    try:
        return pathlib.Path(command("xcode-select", "-p", env=clean_env()))
    except (subprocess.SubprocessError, FileNotFoundError):
        cannot_verify("xcode-select is unavailable, so there is no simulator to drive")


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
    shutil.copytree(developer / "Library/Xcode/Agents/XCTRunner.app", runner)
    plist = runner / "Info.plist"
    subprocess.run(["plutil", "-replace", "CFBundleExecutable", "-string", "XCTRunner", str(plist)],
                   check=True, capture_output=True)
    subprocess.run(["plutil", "-replace", "CFBundleIdentifier", "-string", RUNNER_IDENTIFIER, str(plist)],
                   check=True, capture_output=True)
    subprocess.run(["plutil", "-replace", "CFBundleName", "-string", RUNNER_NAME, str(plist)],
                   check=True, capture_output=True)
    leftover = [line for line in command("plutil", "-p", str(plist)).splitlines() if "$(" in line]
    if leftover:
        cannot_verify("the runner's Info.plist still holds unsubstituted placeholders: "
                      + "; ".join(leftover))

    frameworks = runner / "Frameworks"
    frameworks.mkdir()
    for name in FRAMEWORKS:
        shutil.copytree(developer / f"Library/Frameworks/{name}.framework", frameworks / f"{name}.framework")
    for name in PRIVATE_FRAMEWORKS:
        shutil.copytree(developer / f"Library/PrivateFrameworks/{name}.framework",
                        frameworks / f"{name}.framework")
    shutil.copy2(developer / f"usr/lib/{SWIFT_SUPPORT}", frameworks / SWIFT_SUPPORT)
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


def write_test_run(out, app, bundle, runner, bundle_identifier, run_identifier, taps, region):
    """Describe the run to xcodebuild.

    `TestHostPath` is the runner (for UI tests, the runner *is* the host) and
    `UITargetAppPath` is the application under test. The bundle identifier
    reaches the probe through the test host's environment, so the same compiled
    bundle can be pointed at a different application without rebuilding, and the
    run identifier gives the probe a directory no earlier run can have written
    to. The tap points and the compared region travel the same way, because both
    are geometry that belongs to the application under test.
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
                                         "FLUI_IOS_PROBE_REGION": region},
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
    parser.add_argument("--target-tap", default="0.5,0.138",
                        help="normalized 'dx,dy' that must hit a widget "
                             "(default: the Material demo's first list row)")
    parser.add_argument("--empty-tap", default="0.5,0.035",
                        help="normalized 'dx,dy' that must hit nothing "
                             "(default: the Material demo's app bar title area)")
    parser.add_argument("--region", default="0.0,0.06,1.0,0.94",
                        help="compared region as normalized 'left,top,right,bottom'. It MUST "
                             "contain the pixels the target tap is meant to change: a region "
                             "that crops them out makes a delivered touch and a lost one look "
                             "identical, and the run then reports a lost touch that was never "
                             "lost (default: full width, status bar and home indicator excluded)")
    options = parser.parse_args()

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
          f"output={options.output}")

    with tempfile.TemporaryDirectory(prefix="flui-ios-input-") as staging:
        staged = pathlib.Path(staging) / app.name
        shutil.copytree(app, staged)
        runner = build_runner(platform, pathlib.Path(staging), target, sdk)
        bundle = build_test_bundle(platform, pathlib.Path(staging), target, sdk)
        plan = write_test_run(pathlib.Path(staging), staged, bundle, runner,
                              bundle_identifier, run_identifier,
                              (options.target_tap, options.empty_tap), options.region)
        executed = subprocess.run(
            ["xcodebuild", "test-without-building", "-xctestrun", str(plan),
             "-destination", f"platform=iOS Simulator,id={options.udid}",
             "-resultBundlePath", str(result_bundle)],
            capture_output=True, text=True, env=clean_env(), timeout=1800,
            cwd=staging)
    with (options.output / "xcodebuild.log").open("w") as log:
        log.write(executed.stdout + executed.stderr)

    # The run installs the runner, so its container is read after the fact.
    text = None
    located = container_of(options.udid, RUNNER_IDENTIFIER)
    if located is not None:
        written = located / "tmp/flui-ios-input" / run_identifier
        found = written / "report.txt"
        if found.exists():
            text = found.read_text()
            for artifact in written.iterdir():
                shutil.copy2(artifact, evidence / artifact.name)
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
