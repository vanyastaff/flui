// An XCUITest probe for real touch input and for state that survives a real
// background/foreground round trip on iOS.
//
// Why a UI test bundle exists here at all: nothing else can put a `UITouch`
// into the application. `xcrun simctl` has no touch subcommand, and driving the
// Simulator window through host UI automation needs the Accessibility grant
// (and photographs the host's desktop). XCUITest synthesises touches inside the
// simulator through the platform's own automation channel, so the touch is real
// and the host needs no desktop permission — which is also why this probe can
// run on a machine where desktop scripting is denied.
//
// The oracle is pixels, and it has to be: the framework publishes no
// accessibility tree on iOS (`flui-platform`'s `a11y` bridge covers AT-SPI, UIA
// and NSAccessibility, not UIKit), so a UI test cannot read a widget's value by
// identifier. What it can do is photograph the screen and compare. That is the
// same lesson `just macos-launch-render` records — a window can exist, hold a
// live frame pump, and still show nothing — so "the app logged state" is not
// evidence that the state reached the display, and only the pixels are.
//
// The probe therefore measures four things in one launch:
//
//   tap       a real touch on a card changes the displayed selection
//   resume    Home, then return, and the displayed selection is still there
//   relaunch  a fresh launch shows the initial selection again — the control
//             that proves `resume` could have failed rather than being an
//             assertion that screenshots are always equal
//   no-tap    a real touch that lands on no target changes nothing — the
//             control that proves `tap` discriminates a hit from any touch
//
// Comparison is exact byte equality of the content region (the whole screen
// minus the status bar and the home indicator), because the status-bar clock
// changes on its own and would otherwise make every pair differ. Each settled
// stage writes its screen and the exact region that was hashed, into a
// directory named for this run (see `runIdentifier`).
//
// The verdict is not asserted with XCTAssert: a test failure and a harness that
// could not take the measurement are different results and must not share one
// exit path. The probe writes its measurements and one final `PASS` / `FAIL` /
// `CANNOT_VERIFY` line into its container's `tmp/`, and
// `scripts/check-ios-input.py` reads that file and decides.

import XCTest
import ImageIO
import UniformTypeIdentifiers

/// Content region compared between stages, as a fraction of the screen.
///
/// The status bar is excluded because its clock ticks between stages; the home
/// indicator is excluded because it is drawn by the system on top of whatever
/// the application rendered. It is a rectangle rather than a full-width band
/// because an application may draw under the status bar — the generated counter
/// puts its only changing text immediately below the clock — and a band that
/// cut it off would report a delivered touch as a lost one. A narrower region
/// excludes the clock horizontally instead, where a horizontal cut is not
/// available. Callers set these to cover what the tap is meant to change.
private let defaultRegion = "0.0,0.06,1.0,0.94"

private func region(_ variable: String, _ fallback: String) -> (left: Double, top: Double,
                                                                right: Double, bottom: Double) {
    let text = ProcessInfo.processInfo.environment[variable] ?? fallback
    let parts = text.split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
    guard parts.count == 4 else { return (0, 0.06, 1, 0.94) }
    return (parts[0], parts[1], parts[2], parts[3])
}

private let contentRegion = region("FLUI_IOS_PROBE_REGION", defaultRegion)

/// The bundle under test and the two tap points, supplied by the checker
/// through the xctestrun's `EnvironmentVariables`, which reach the test host.
/// Defaults keep the probe runnable by hand against the Material demo.
private let defaultBundle = "dev.flui.ios-demo"

/// A tap that must reach a widget, and a tap that must not.
///
/// These are geometry, not policy, so they come in as `"dx,dy"` normalized
/// offsets: which point hits a target is a property of the application under
/// test, and a gate that hard-coded the Material demo's row position could only
/// ever measure that one application. The defaults are the demo's first list
/// row, and the app bar's title area, which has no handler.
private let defaultTargetTap = "0.5,0.138"
private let defaultEmptyTap = "0.5,0.035"

private func tapPoint(_ variable: String, _ fallback: String) -> CGVector {
    let text = ProcessInfo.processInfo.environment[variable] ?? fallback
    let parts = text.split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
    guard parts.count == 2 else { return CGVector(dx: 0.5, dy: 0.5) }
    return CGVector(dx: parts[0], dy: parts[1])
}

/// The target tap reaches a widget; the empty tap reaches none.
private let targetTap = tapPoint("FLUI_IOS_PROBE_TARGET_TAP", defaultTargetTap)
private let emptyTap = tapPoint("FLUI_IOS_PROBE_EMPTY_TAP", defaultEmptyTap)

/// Whether the resume stage must prove the application was still *live* after
/// the round trip, by advancing its state with a second tap.
///
/// Equality of two screenshots cannot carry a retention claim on its own: iOS
/// keeps a snapshot of the pre-Home frame and displays it again on return, so
/// "the pixels after Home/return equal the pixels before it" holds whether the
/// application retained anything or was killed and silently relaunched onto its
/// own snapshot. The snapshot is, by construction, the frame taken before Home.
/// What a snapshot cannot do is *advance*: a second tap after the return only
/// changes the display if a live widget received it, and only leaves the
/// initial state again if the process is the one that was already running
/// rather than a fresh launch. Subjects whose target tap advances state (an
/// incrementing counter) can opt into that stronger oracle; subjects whose tap
/// is idempotent (selecting an already-selected row) cannot, and their report
/// says so instead of implying a liveness proof they never made.
private let postReturnTap = ["1", "true", "yes"].contains(
    (ProcessInfo.processInfo.environment["FLUI_IOS_PROBE_POST_RETURN_TAP"] ?? "").lowercased())

/// Names this run's evidence directory. The runner's container survives
/// reinstallation, so evidence written to a fixed path is read back as the next
/// run's own — a stale screenshot of a stage that never happened is
/// indistinguishable from a real one. A per-run directory makes that
/// impossible rather than unlikely.
private let runIdentifier =
    ProcessInfo.processInfo.environment["FLUI_IOS_PROBE_RUN"] ?? "manual"

/// How long a stage waits for the screen to stop changing, and how far apart
/// two samples are taken while it does.
private let settleTimeout = 25.0
private let settleInterval = 0.4

/// The ink share below which the first stage is treated as a screen with
/// nothing on it. The Material demo measures 2.81 % and the generated counter
/// 1.06 % over their compared regions, so this is a floor against a blank
/// screen rather than a quality bar.
private let emptyScreenInk = 0.05

/// A stage's measurement. `crc` is the identity compared between stages; `ink`
/// is the share of sampled pixels outside the region's dominant colour,
/// reported so a result carries *how much* was on screen and not only that two
/// images matched — the same discipline `just macos-launch-render` follows,
/// where a 99.94 %-one-colour window needed the ink figure to be readable.
private struct Measurement {
    let crc: UInt32
    let ink: Double
    let settled: Bool
}

private let crcLookup: [UInt32] = {
    var table = [UInt32](repeating: 0, count: 256)
    for index in 0..<256 {
        var value = UInt32(index)
        for _ in 0..<8 {
            value = (value & 1) != 0 ? 0xEDB8_8320 ^ (value >> 1) : value >> 1
        }
        table[index] = value
    }
    return table
}()

final class FluiIOSInputProbe: XCTestCase {
    private let bundleIdentifier =
        ProcessInfo.processInfo.environment["FLUI_IOS_PROBE_BUNDLE"] ?? defaultBundle
    private var lines: [String] = []

    // MARK: - the measurement sequence

    func testRealTouchAndResume() {
        let app = XCUIApplication(bundleIdentifier: bundleIdentifier)
        app.launch()
        guard app.wait(for: .runningForeground, timeout: 30) else {
            return finish("CANNOT_VERIFY", "the application never reached the foreground")
        }
        // Let the first frame land before the region is sampled: the settle
        // poll below cannot tell the application apart from what it replaced.
        Thread.sleep(forTimeInterval: 3)

        let initial = measure("initial")
        guard initial.settled else {
            return finish("CANNOT_VERIFY", "the screen never stopped changing at 'initial'")
        }
        // A screen with nothing drawn on it cannot answer "does a touch reach a
        // widget": no widget was there to reach, so a tap that changes nothing
        // says nothing about the touch path. This is the same distinction the
        // macOS launch-route gate draws with its colour oracle, and it is not
        // hypothetical — a run of this probe once caught the application before
        // it had drawn anything (ink 0.00%) and reported a lost touch that had
        // never been delivered anywhere. The floor is deliberately low: it
        // separates "drew nothing" from "drew something", not "good" from "bad".
        guard initial.ink >= emptyScreenInk else {
            return finish("CANNOT_VERIFY", "the application had not drawn anything when the "
                          + "first stage settled (ink \(String(format: "%.2f", initial.ink))%), so "
                          + "there was no widget to tap and nothing to compare")
        }

        app.coordinate(withNormalizedOffset: targetTap).tap()
        let tapped = measure("tapped")
        guard tapped.settled else {
            return finish("CANNOT_VERIFY", "the screen never stopped changing after the tap")
        }

        XCUIDevice.shared.press(.home)
        let backgrounded = app.wait(for: .runningBackground, timeout: 20)
        app.activate()
        let returnedForeground = app.wait(for: .runningForeground, timeout: 20)
        Thread.sleep(forTimeInterval: 1)
        let resumed = measure("resumed")
        guard resumed.settled else {
            return finish("CANNOT_VERIFY", "the screen never stopped changing after the return")
        }

        // The liveness half of the round trip, for subjects whose tap advances
        // state. A system snapshot is the pre-Home frame, so it satisfies an
        // equality comparison while proving nothing about the process; a touch
        // that changes the display *and* leaves the initial state again cannot
        // be a snapshot, and cannot be a fresh launch either — a relaunch would
        // have reset the state the tap is about to change.
        var resumedTapped: Measurement?
        if postReturnTap {
            app.coordinate(withNormalizedOffset: targetTap).tap()
            let advanced = measure("resumed-tapped")
            guard advanced.settled else {
                return finish("CANNOT_VERIFY", "the screen never stopped changing after the "
                              + "post-return tap")
            }
            resumedTapped = advanced
        }

        app.terminate()
        app.launch()
        guard app.wait(for: .runningForeground, timeout: 30) else {
            return finish("CANNOT_VERIFY", "the application never reached the foreground on relaunch")
        }
        Thread.sleep(forTimeInterval: 3)
        let relaunched = measure("relaunched")
        guard relaunched.settled else {
            return finish("CANNOT_VERIFY", "the screen never stopped changing after the relaunch")
        }

        app.coordinate(withNormalizedOffset: emptyTap).tap()
        let emptyTapped = measure("empty-tapped")
        guard emptyTapped.settled else {
            return finish("CANNOT_VERIFY", "the screen never stopped changing after the empty tap")
        }

        // MARK: - verdicts

        var failures: [String] = []
        if initial.crc == tapped.crc {
            failures.append("the tap changed nothing on screen: a real touch did not reach a widget "
                            + "(the region was identical before and after)")
        }
        if !backgrounded {
            failures.append("the application did not report itself backgrounded after Home")
        }
        if !returnedForeground {
            failures.append("the application did not return to the foreground")
        }
        if backgrounded, returnedForeground, resumed.crc != tapped.crc {
            failures.append("the displayed state after Home/return differs from the displayed state "
                            + "before it: state did not survive the round trip")
        }
        if let advanced = resumedTapped {
            // A snapshot of the pre-Home frame cannot respond to a new touch,
            // and a fresh launch would be showing the initial state the tap is
            // meant to move away from — so both halves together are the
            // liveness proof the equality comparison above cannot give.
            if advanced.crc == resumed.crc {
                failures.append("a touch after Home/return changed nothing on screen: whatever is "
                                + "displayed is not a live widget (a system snapshot of the "
                                + "pre-Home frame satisfies the comparison above)")
            }
            if advanced.crc == initial.crc {
                failures.append("the post-return tap left the screen in the initial state, so the "
                                + "application was relaunched rather than resumed and the retained "
                                + "state proved nothing")
            }
        }
        if relaunched.crc == tapped.crc {
            failures.append("a fresh launch still displays the tapped state, so the return comparison "
                            + "above cannot fail and proves nothing")
        }
        if relaunched.crc != initial.crc {
            failures.append("a fresh launch did not display the initial state, so it is not the "
                            + "baseline the return comparison is measured against")
        }
        if emptyTapped.crc != relaunched.crc {
            failures.append("a tap at a point with no target changed the screen, so the tap comparison "
                            + "does not distinguish a hit from any touch")
        }

        record("tap=\(initial.crc == tapped.crc ? "no-change" : "changed") "
               + "initial=\(hex(initial.crc)) tapped=\(hex(tapped.crc))")
        record("resume=\(resumed.crc == tapped.crc ? "retained" : "lost") "
               + "resumed=\(hex(resumed.crc)) "
               + "oracle=\(postReturnTap ? "display+live-touch" : "display-equality-only")")
        if let advanced = resumedTapped {
            record("resume-tap=\(advanced.crc == resumed.crc ? "no-change" : "changed") "
                   + "resumedTapped=\(hex(advanced.crc))")
        }
        record("relaunch=\(relaunched.crc == initial.crc ? "reset" : "not-initial") "
               + "relaunched=\(hex(relaunched.crc))")
        record("no-tap=\(emptyTapped.crc == relaunched.crc ? "unchanged" : "changed") "
               + "emptyTapped=\(hex(emptyTapped.crc))")
        record("region=x[\(percent(contentRegion.left))..\(percent(contentRegion.right))] "
               + "y[\(percent(contentRegion.top))..\(percent(contentRegion.bottom))] "
               + "ink(initial)=\(String(format: "%.2f", initial.ink))% "
               + "ink(tapped)=\(String(format: "%.2f", tapped.ink))%")
        record("tapPoints=target(\(targetTap.dx),\(targetTap.dy)) empty(\(emptyTap.dx),\(emptyTap.dy))")

        if failures.isEmpty {
            // The parenthetical is not decoration: equality alone cannot
            // exclude a system snapshot of the pre-Home frame, so a pass that
            // rests on equality must say which oracle carried it rather than
            // letting the sentence read as a liveness proof.
            let carried = postReturnTap
                ? "a touch after the return still reached a live widget"
                : "the display matched, by equality alone"
            finish("PASS", "real touch changed the display, the change survived Home/return "
                   + "(\(carried)), and both controls behaved: a fresh launch showed the initial "
                   + "state and a tap on no target did not")
        } else {
            finish("FAIL", failures.joined(separator: "; "))
        }
    }

    // MARK: - measuring

    /// Sample the content region until two consecutive samples agree.
    ///
    /// A fixed sleep would be a guess about how long a frame takes; this is a
    /// measurement of it. It is also why an animating application reports
    /// CANNOT_VERIFY instead of a verdict: a screen that never stops changing
    /// cannot be compared between stages, so no result is available.
    private func measure(_ label: String) -> Measurement {
        var previous = digest(XCUIScreen.main.screenshot())
        let deadline = Date().addingTimeInterval(settleTimeout)
        while Date() < deadline {
            Thread.sleep(forTimeInterval: settleInterval)
            let shot = XCUIScreen.main.screenshot()
            let current = digest(shot)
            if current.crc == previous.crc {
                record("\(label) settled at \(hex(current.crc)) "
                       + "ink=\(String(format: "%.2f", current.ink))%")
                write(shot, current.region, label)
                return Measurement(crc: current.crc, ink: current.ink, settled: true)
            }
            previous = current
        }
        record("\(label) NEVER SETTLED (last \(hex(previous.crc)))")
        return Measurement(crc: previous.crc, ink: previous.ink, settled: false)
    }

    /// Write a settled stage's evidence: the screen as it looked, and the exact
    /// region that was hashed.
    ///
    /// Both are kept because they answer different questions. The screen shows
    /// what the application drew, and is what a person reads to see a change;
    /// the region is what the comparison actually covered, so a reader can
    /// check the hash against the same bytes rather than trusting that the
    /// insets cut only the status bar.
    ///
    /// A failed write is recorded rather than discarded. iOS may purge an
    /// application's `tmp/` while it is not running, and this probe runs inside
    /// a test host that is backgrounded for the whole session — so evidence can
    /// disappear for reasons that have nothing to do with the measurement, and
    /// a run whose evidence is incomplete must say so instead of looking like a
    /// run that produced none.
    private func write(_ shot: XCUIScreenshot, _ region: CGImage?, _ label: String) {
        let directory = evidenceDirectory()
        var problems: [String] = []
        do {
            try shot.pngRepresentation.write(to: directory.appendingPathComponent("\(label).png"))
        } catch {
            problems.append("screen: \(error)")
        }
        if let region, let encoded = encodePNG(region) {
            do {
                try encoded.write(to: directory.appendingPathComponent("\(label)-region.png"))
            } catch {
                problems.append("region: \(error)")
            }
        } else {
            problems.append("region: the crop could not be encoded")
        }
        if !problems.isEmpty {
            record("\(label) EVIDENCE INCOMPLETE (\(problems.joined(separator: "; ")))")
        }
    }

    private func encodePNG(_ image: CGImage) -> Data? {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            data, UTType.png.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { return nil }
        return data as Data
    }

    /// Hash, ink share and the region of the screen that was hashed.
    ///
    /// The crop is redrawn rather than hashed through `cropping(to:)`'s own
    /// buffer: a crop can share its provider with the full image, so hashing
    /// the provider's bytes would compare whole screens while appearing to
    /// compare a band. Redrawing into a context whose size is the crop's makes
    /// the bytes exactly the region.
    private func digest(_ shot: XCUIScreenshot) -> (crc: UInt32, ink: Double, region: CGImage?) {
        guard let full = shot.image.cgImage,
              let region = full.cropping(to: CGRect(
                  x: Int(contentRegion.left * Double(full.width)),
                  y: Int(contentRegion.top * Double(full.height)),
                  width: Int((contentRegion.right - contentRegion.left) * Double(full.width)),
                  height: Int((contentRegion.bottom - contentRegion.top) * Double(full.height)))),
              let context = CGContext(data: nil, width: region.width, height: region.height,
                                      bitsPerComponent: 8, bytesPerRow: region.width * 4,
                                      space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let base = context.data
        else {
            return (0, 0, nil)
        }
        context.draw(region, in: CGRect(x: 0, y: 0, width: region.width, height: region.height))
        let bytes = base.assumingMemoryBound(to: UInt8.self)
        let count = region.width * region.height * 4

        var crc: UInt32 = 0xFFFF_FFFF
        for index in 0..<count {
            crc = crcLookup[Int((crc ^ UInt32(bytes[index])) & 0xFF)] ^ (crc >> 8)
        }

        var colours: [UInt32: Int] = [:]
        var sampled = 0
        // Every seventh pixel is enough for the colour census and keeps the
        // census from dominating the run; the hash still covers every byte.
        for offset in stride(from: 0, to: count, by: 4 * 7) {
            let red = UInt32(bytes[offset]) / 32, green = UInt32(bytes[offset + 1]) / 32
            let blue = UInt32(bytes[offset + 2]) / 32, alpha = UInt32(bytes[offset + 3]) / 32
            colours[red << 24 | green << 16 | blue << 8 | alpha, default: 0] += 1
            sampled += 1
        }
        let dominant = colours.values.max() ?? sampled
        let ink = sampled > 0 ? 100 * Double(sampled - dominant) / Double(sampled) : 0
        return (crc ^ 0xFFFF_FFFF, ink, region)
    }

    // MARK: - reporting

    /// This run's own directory inside the runner container's `tmp/`.
    private func evidenceDirectory() -> URL {
        let directory = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("flui-ios-input", isDirectory: true)
            .appendingPathComponent(runIdentifier, isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    private func record(_ line: String) {
        lines.append(line)
        writeReport()
    }

    /// Record the verdict and stop.
    ///
    /// Nothing is asserted here on purpose: `check-ios-input.py` reads the
    /// report and decides. A failed assertion would give the run one signal
    /// where there are two — "the framework lost state" and "the harness never
    /// took the measurement" — and those must not share an exit path.
    private func finish(_ verdict: String, _ detail: String) {
        record("\(verdict) \(detail)")
    }

    private func writeReport() {
        try? (lines.joined(separator: "\n") + "\n")
            .write(to: evidenceDirectory().appendingPathComponent("report.txt"),
                   atomically: true, encoding: .utf8)
    }

    private func hex(_ value: UInt32) -> String {
        String(format: "%08x", value)
    }

    private func percent(_ value: Double) -> String {
        String(format: "%.3f", value)
    }
}
