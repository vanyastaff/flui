#!/usr/bin/env python3
"""Run `examples/workload_probe.rs` in release and gate it on declared budgets.

This is the evidence collector behind docs/BETA.md's "Performance and
resilience" row: it builds the probe in release, runs it on a real window
(RUST_LOG=warn), samples RSS every 0.5s, collects the probe's own per-phase
JSON summary lines from stdout, and checks each against a budget declared
below — chosen *before* the first run of this script. Changing any budget
constant below requires a docs/BETA.md note explaining why; do not tune a
budget just to make a run pass. A failing budget is reported as a finding,
with the raw numbers, not silently loosened.

macOS-only: the probe measures this substrate's AppKit-backed desktop
pipeline, and `sw_vers`/`sysctl` below are macOS tools. Run elsewhere and
this prints a skip line and exits 0.

Usage: python3 scripts/check-macos-workload.py
Needs a GUI session — the probe opens a real, visible window and closes it
itself when done.
"""

from __future__ import annotations

import json
import os
import platform
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

# =============================================================================
# Budgets — declared here, before the first run of this script. Each one is a
# claim this workload makes about the framework's real cost; a change to any
# of these numbers needs a matching docs/BETA.md note, not a silent edit.
# =============================================================================
SCROLL_P99_MAX_PERIODS = 2.0
SCROLL_OVER_2P_FRAC_MAX = 0.01
TYPE_P99_MAX_PERIODS = 2.0
RSS_GROWTH_MAX_FRACTION = 0.10
IDLE_FRAMES_MAX = 5
# RSS growth is measured from this point in the run, not from the first
# sample: the first run (2026-09-22, docs/BETA.md) showed RSS climbing from
# 75.6 MiB at 1 s to 199.6 MiB at 2.2 s — the GPU stack, glyph atlas and
# first laid-out screen being allocated — and then flat to within 1 % for
# the remaining 33 s. A baseline inside that ramp measures startup, not
# growth; five seconds is well past it on this host and still inside the
# 20 s scroll phase, so the comparison spans real work.
RSS_BASELINE_AT_SECONDS = 5.0

# Best-effort process ceiling: scroll (default 20s) + type (default ~500
# ticks, well under 10s at any plausible frame rate) + idle (5s) + generous
# startup/teardown slack. Not a budget — just a hang guard.
PROCESS_TIMEOUT_SECONDS = 180
RSS_SAMPLE_INTERVAL_SECONDS = 0.5

REPO_ROOT = Path(__file__).resolve().parents[1]
BINARY_PATH = REPO_ROOT / "target" / "release" / "examples" / "workload_probe"


def is_macos() -> bool:
    return platform.system() == "Darwin"


def clean_build_env() -> dict:
    """A copy of the environment with Apple Python's injected `SDKROOT`
    removed.

    The system `/usr/bin/python3` sets `SDKROOT` (and sometimes
    `DEVELOPER_DIR`) in its own process environment, pointing at whatever SDK
    Python itself was built against — on this host that resolves to a broken
    Xcode beta SDK (`MacOSX27.0.sdk`, whose `libSystem.B.tbd` lists an
    architecture this toolchain's `ld` does not understand). A `cargo build`
    launched as this script's *child* inherits that `SDKROOT` and fails to
    link, even though the identical `cargo build` succeeds in an ordinary
    shell, which never has `SDKROOT` set. Stripping it here lets `cargo`/`cc`
    fall back to `xcrun`'s own default SDK resolution, matching a plain
    shell.
    """
    env = os.environ.copy()
    env.pop("SDKROOT", None)
    env.pop("DEVELOPER_DIR", None)
    return env


def run_text(cmd: list[str]) -> str:
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=10, check=False)
        return result.stdout.strip()
    except Exception as error:  # noqa: BLE001 - best-effort diagnostics only
        return f"unavailable ({error})"


DISPLAY_PERIOD_SWIFT = """\
import CoreGraphics
let mode = CGDisplayCopyDisplayMode(CGMainDisplayID())
print(mode?.refreshRate ?? 0)
"""


def get_display_period_ms() -> tuple[float | None, str]:
    """The main display's refresh period in ms, and where it came from.

    Asks CoreGraphics (`CGDisplayCopyDisplayMode(CGMainDisplayID())`) through
    a one-line Swift helper compiled into `target/workload/`, since
    `system_profiler` reports no refresh rate for a built-in panel and the
    probe itself cannot reach `PlatformWindow::refresh_period` from the
    facade (its module doc's "Honest gaps"). The value is handed to the
    probe as `FLUI_WORKLOAD_PERIOD_MS` unless the operator already set one,
    so the period-relative budgets below are stated against the real panel.
    `None` when the helper cannot be built or answers 0 (a display whose
    mode reports no rate); the budgets then use the probe's default.
    """
    out_dir = REPO_ROOT / "target" / "workload"
    out_dir.mkdir(parents=True, exist_ok=True)
    source = out_dir / "display_period.swift"
    binary = out_dir / "display_period"
    try:
        if not binary.exists() or source.read_text() != DISPLAY_PERIOD_SWIFT:
            source.write_text(DISPLAY_PERIOD_SWIFT)
            subprocess.run(
                ["xcrun", "swiftc", "-O", str(source), "-o", str(binary)],
                env=clean_build_env(),
                capture_output=True,
                text=True,
                timeout=120,
                check=True,
            )
        hertz = float(run_text([str(binary)]))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        return None, f"unavailable ({error})"
    if hertz <= 0:
        return None, "CGDisplayCopyDisplayMode reported no refresh rate"
    return 1000.0 / hertz, "CGDisplayCopyDisplayMode(CGMainDisplayID())"


def sample_rss_kib(pid: int) -> int | None:
    try:
        result = subprocess.run(
            ["ps", "-o", "rss=", "-p", str(pid)],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
        text = result.stdout.strip()
        return int(text) if text else None
    except Exception:  # noqa: BLE001 - a missed sample is just a gap
        return None


def check(results: list[dict], name: str, passed: bool, detail: str) -> None:
    status = "PASS" if passed else "FAIL"
    print(f"{status}: {name} — {detail}")
    results.append({"name": name, "passed": passed, "detail": detail})


def main() -> int:
    if not is_macos():
        print(f"SKIP: check-macos-workload.py is macOS-only (platform is {platform.system()})")
        return 0

    build_cmd = [
        "cargo",
        "build",
        "--locked",
        "--release",
        "--example",
        "workload_probe",
        "--features",
        "material",
    ]
    print(f"building: {' '.join(build_cmd)}")
    build = subprocess.run(build_cmd, cwd=REPO_ROOT, env=clean_build_env(), check=False)
    if build.returncode != 0:
        print("FAIL: build — cargo build did not succeed")
        return 1
    if not BINARY_PATH.exists():
        print(f"FAIL: build — expected binary missing at {BINARY_PATH}")
        return 1

    sw_vers = run_text(["sw_vers"])
    cpu_brand = run_text(["sysctl", "-n", "machdep.cpu.brand_string"])
    display_period_ms, display_period_source = get_display_period_ms()
    print(f"sw_vers:\n{sw_vers}")
    print(f"cpu: {cpu_brand}")
    print(f"display period: {display_period_ms} ms ({display_period_source})")

    env = clean_build_env()
    env["RUST_LOG"] = "warn"
    if display_period_ms is not None and "FLUI_WORKLOAD_PERIOD_MS" not in env:
        env["FLUI_WORKLOAD_PERIOD_MS"] = f"{display_period_ms:.4f}"
    workload_env = {
        key: env[key]
        for key in (
            "FLUI_WORKLOAD_SCROLL_SECONDS",
            "FLUI_WORKLOAD_TYPE_CHARS",
            "FLUI_WORKLOAD_PERIOD_MS",
        )
        if key in env
    }

    print(f"running: {BINARY_PATH}")
    process = subprocess.Popen(
        [str(BINARY_PATH)],
        cwd=REPO_ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )

    phase_lines: list[dict] = []
    stdout_lines_seen: list[str] = []

    def read_stdout() -> None:
        assert process.stdout is not None
        for raw_line in process.stdout:
            line = raw_line.strip()
            if not line:
                continue
            stdout_lines_seen.append(line)
            try:
                parsed = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict) and "phase" in parsed:
                parsed["_observed_at_wall_s"] = time.time()
                phase_lines.append(parsed)

    reader = threading.Thread(target=read_stdout, daemon=True)
    reader.start()

    # stderr is drained as it arrives too: a probe that logs more than the
    # pipe buffer (~64 KiB) would otherwise block on write and never exit,
    # and be reported as a hang it did not have.
    stderr_chunks: list[str] = []

    def read_stderr() -> None:
        assert process.stderr is not None
        for raw_line in process.stderr:
            stderr_chunks.append(raw_line)

    stderr_reader = threading.Thread(target=read_stderr, daemon=True)
    stderr_reader.start()

    rss_samples_kib: list[tuple[float, int]] = []
    start_time = time.time()
    timed_out = False
    while True:
        if process.poll() is not None:
            break
        elapsed = time.time() - start_time
        if elapsed > PROCESS_TIMEOUT_SECONDS:
            timed_out = True
            process.kill()
            break
        rss = sample_rss_kib(process.pid)
        if rss is not None:
            rss_samples_kib.append((elapsed, rss))
        time.sleep(RSS_SAMPLE_INTERVAL_SECONDS)

    process.wait(timeout=10)
    reader.join(timeout=10)
    stderr_reader.join(timeout=10)
    stderr_output = "".join(stderr_chunks)

    results: list[dict] = []

    if timed_out:
        check(results, "process_completed", False, f"killed after {PROCESS_TIMEOUT_SECONDS}s without exiting")
    elif process.returncode != 0:
        check(results, "process_completed", False, f"exit code {process.returncode}")
    else:
        check(results, "process_completed", True, "exit code 0")

    scroll = next((p for p in phase_lines if p.get("phase") == "scroll"), None)
    type_phase = next((p for p in phase_lines if p.get("phase") == "type"), None)
    idle_phase = next((p for p in phase_lines if p.get("phase") == "idle"), None)

    if scroll is not None:
        period_ms = float(scroll.get("period_ms", 16.67))
        threshold_ms = SCROLL_P99_MAX_PERIODS * period_ms
        p99_ms = float(scroll.get("p99_ms", float("inf")))
        check(
            results,
            "scroll_p99",
            p99_ms <= threshold_ms,
            f"p99={p99_ms:.3f}ms budget<={threshold_ms:.3f}ms "
            f"({SCROLL_P99_MAX_PERIODS}x period_ms={period_ms:.3f}, "
            f"period_source={scroll.get('period_source')})",
        )
        frames = int(scroll.get("frames", 0))
        over_2p = int(scroll.get("over_2p_frames", 0))
        fraction = (over_2p / frames) if frames else 1.0
        check(
            results,
            "scroll_over_2p_frames",
            fraction <= SCROLL_OVER_2P_FRAC_MAX,
            f"over_2p_frames={over_2p}/{frames} ({fraction:.3%}) "
            f"budget<={SCROLL_OVER_2P_FRAC_MAX:.1%}",
        )
    else:
        check(results, "scroll_p99", False, "no scroll summary line observed on stdout")
        check(results, "scroll_over_2p_frames", False, "no scroll summary line observed on stdout")

    if type_phase is not None:
        period_ms = float(type_phase.get("period_ms", 16.67))
        threshold_ms = TYPE_P99_MAX_PERIODS * period_ms
        p99_ms = float(type_phase.get("p99_ms", float("inf")))
        check(
            results,
            "type_p99",
            p99_ms <= threshold_ms,
            f"p99={p99_ms:.3f}ms budget<={threshold_ms:.3f}ms "
            f"({TYPE_P99_MAX_PERIODS}x period_ms={period_ms:.3f})",
        )
    else:
        check(results, "type_p99", False, "no type summary line observed on stdout")

    if idle_phase is not None:
        frames = int(idle_phase.get("frames", 0))
        check(
            results,
            "idle_frames",
            frames <= IDLE_FRAMES_MAX,
            f"frames={frames} budget<={IDLE_FRAMES_MAX}",
        )
    else:
        check(results, "idle_frames", False, "no idle summary line observed on stdout")

    rss_summary_mib = {"start": None, "peak": None, "end": None}
    if rss_samples_kib:
        rss_summary_mib["start"] = rss_samples_kib[0][1] / 1024.0
        rss_summary_mib["peak"] = max(kib for _, kib in rss_samples_kib) / 1024.0
        rss_summary_mib["end"] = rss_samples_kib[-1][1] / 1024.0

    baseline = next(
        ((elapsed, kib) for elapsed, kib in rss_samples_kib if elapsed >= RSS_BASELINE_AT_SECONDS),
        None,
    )
    if baseline is not None and rss_samples_kib[-1][0] > baseline[0]:
        baseline_at, baseline_kib = baseline
        end_kib = rss_samples_kib[-1][1]
        growth_fraction = (end_kib - baseline_kib) / baseline_kib if baseline_kib else float("inf")
        check(
            results,
            "rss_growth",
            growth_fraction <= RSS_GROWTH_MAX_FRACTION,
            f"baseline={baseline_kib / 1024.0:.1f}MiB (at {baseline_at:.1f}s) "
            f"end={end_kib / 1024.0:.1f}MiB growth={growth_fraction:.1%} "
            f"budget<={RSS_GROWTH_MAX_FRACTION:.0%}",
        )
    else:
        check(
            results,
            "rss_growth",
            False,
            f"no RSS sample at or after {RSS_BASELINE_AT_SECONDS}s with a later end sample",
        )

    overall_pass = all(r["passed"] for r in results)
    print(f"WORKLOAD={'PASS' if overall_pass else 'FAIL'}")
    print(
        "RSS: start={start} peak={peak} end={end} MiB".format(
            start=f"{rss_summary_mib['start']:.1f}" if rss_summary_mib["start"] is not None else "n/a",
            peak=f"{rss_summary_mib['peak']:.1f}" if rss_summary_mib["peak"] is not None else "n/a",
            end=f"{rss_summary_mib['end']:.1f}" if rss_summary_mib["end"] is not None else "n/a",
        )
    )

    out_dir = REPO_ROOT / "target" / "workload"
    out_dir.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_path = out_dir / f"{timestamp}.json"
    payload = {
        "timestamp_utc": timestamp,
        "sw_vers": sw_vers,
        "cpu_brand": cpu_brand,
        "display_period_ms": display_period_ms,
        "display_period_source": display_period_source,
        "workload_env_overrides": workload_env,
        "budgets_declared": {
            "scroll_p99_max_periods": SCROLL_P99_MAX_PERIODS,
            "scroll_over_2p_frac_max": SCROLL_OVER_2P_FRAC_MAX,
            "type_p99_max_periods": TYPE_P99_MAX_PERIODS,
            "rss_growth_max_fraction": RSS_GROWTH_MAX_FRACTION,
            "idle_frames_max": IDLE_FRAMES_MAX,
            "rss_baseline_at_seconds": RSS_BASELINE_AT_SECONDS,
        },
        "phases": phase_lines,
        "rss_series_kib": rss_samples_kib,
        "rss_summary_mib": rss_summary_mib,
        "results": results,
        "overall": "PASS" if overall_pass else "FAIL",
        "process_timed_out": timed_out,
        "process_returncode": process.returncode,
        "stdout_lines": stdout_lines_seen,
        "stderr_tail": stderr_output[-4000:],
    }
    out_path.write_text(json.dumps(payload, indent=2))
    print(f"wrote {out_path}")

    return 0 if overall_pass else 1


if __name__ == "__main__":
    sys.exit(main())
