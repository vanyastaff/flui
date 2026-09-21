#!/usr/bin/env python3
"""Drive `flui run`'s worker hot-reload loop end to end on a generated project.

The developer-iteration row of docs/BETA.md asks for evidence that the
documented reload mode applies edits predictably, preserves state per its
contract, survives a failed edit, does nothing while idle, and shuts down
cleanly. This script is that loop, against a project the CLI itself
generates (`flui create --hot-reload`), driven through the CLI's own
machine-readable event stream (`flui --json run`), so the oracle is what the
CLI reports to any tool, not a scrape of its narration.

Stages, each bounded:

  0. `flui run` starts the host; `run.app.start` names its PID and the host
     puts a window on screen (CoreGraphics window list, matched by PID).
  1. EDIT #1 — the worker's `INCREMENT_LABEL` changes AND the edit adds a
     witness to the worker's build function: it bumps the host-owned
     counter and prints `PROBE count=…` on stderr. The CLI must report
     `run.build.done ok=true` and `run.reload kind=hot ok=true`, the host
     PID must be unchanged (a restart would have thrown the state away), and
     the witness must appear — code that did not exist until this reload,
     running inside the host that was started before it.
  2. EDIT #2 — a syntax error; the CLI must report `run.build.done ok=false`
     and `run.reload ok=false`, the host PID must still be alive, and no
     `run.app.exit` may appear.
  3. EDIT #3 — the fix; a successful reload again, same PID, and the witness
     must print a count STRICTLY ABOVE the one after EDIT #1. That is the
     state-preservation proof: the counter is `CounterState`, owned by the
     host's element tree; a value that carried across a reload, a failed
     rebuild and a second reload was preserved, where a restart (or a worker
     that owned its own state) would have started it over at zero. No
     synthetic input is posted: a probe that drives the pointer would hijack
     an operator's mouse, and the witness needs none.
  4. IDLE — `--idle` seconds with no edit: no build or reload event.
  5. SHUTDOWN — SIGINT to `flui run`; it and the host must exit within the
     bound (the CLI reports the interrupt as an `error` event with code 130).

Exit 0 when every stage held; 1 with the first failure named. Everything
the CLI printed is kept at `<work>/run.jsonl`; the generated project at
`<work>/hr_probe`. macOS only: the window oracle is the CoreGraphics list.
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import signal
import subprocess
import sys
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
WINDOW_LIST_SOURCE = ROOT / 'scripts/macos-window-list.swift'
ACTIVATE_SOURCE = ROOT / 'scripts/macos-activate.swift'
LABEL_LINE = 'const INCREMENT_LABEL: &str = "Increment (+1)";'
COUNT_LINE = '    let count = state.count().load(Ordering::Relaxed);'
# Bumps the host-owned counter on every build and reports it: the value the
# NEXT reload's worker reads back is the state the host carried across.
WITNESS = ('    let count = state.count().fetch_add(1, Ordering::Relaxed) + 1;\n'
           '    eprintln!("PROBE count={count}");')
PROBE = re.compile(r'PROBE count=(\d+)')


def fail(message):
    print(f'HOT_RELOAD_LOOP=FAIL: {message}', flush=True)
    sys.exit(1)


def build_helper(source, dest):
    if dest.exists() and dest.stat().st_mtime >= source.stat().st_mtime:
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    # Apple's Python exports SDKROOT for its own SDK; a swiftc child inheriting
    # it resolves against an SDK that need not match the selected toolchain
    # (see check-macos-launch-render.py's clean_env for the same drop).
    env = dict(os.environ)
    env.pop('SDKROOT', None)
    compiled = subprocess.run(['xcrun', 'swiftc', '-O', '-o', str(dest), str(source)],
                              capture_output=True, text=True, env=env)
    if compiled.returncode:
        fail(f'swiftc could not build {source.name}:\n{compiled.stderr}')
    return dest




def window_for_pid(helper, pid):
    listing = subprocess.run([str(helper)], capture_output=True, text=True).stdout
    for line in listing.splitlines():
        if f'pid={pid} ' in line:
            return line.strip()
    return None


class EventStream:
    """The CLI's `--json` stdout, one object per line, collected as it arrives."""

    def __init__(self, process, log):
        self.events = []
        self.lock = threading.Lock()
        self.log = log
        self.thread = threading.Thread(target=self._pump, args=(process,), daemon=True)
        self.thread.start()

    def _pump(self, process):
        for raw in process.stdout:
            line = raw.decode('utf-8', 'replace').rstrip('\n')
            self.log.write(line + '\n')
            self.log.flush()
            if line.startswith('{'):
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    event = {'event': 'raw', 'line': line}
            else:
                # The host's stderr reaches this stream unwrapped today (the
                # CLI wraps stdout as `run.app.log` and passes stderr through);
                # kept as a pseudo-event so the witness is found either way.
                event = {'event': 'raw', 'line': line}
            with self.lock:
                self.events.append(event)

    def snapshot(self):
        with self.lock:
            return list(self.events)

    def wait_for(self, predicate, timeout, since=0):
        """The first event at index >= `since` satisfying `predicate`, or None."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            events = self.snapshot()
            for index in range(since, len(events)):
                if predicate(events[index]):
                    return index, events[index]
            time.sleep(0.2)
        return None


def pid_alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('work', type=pathlib.Path, help='scratch directory (recreated)')
    parser.add_argument('--cli', type=pathlib.Path, default=ROOT / 'target/debug/flui',
                        help='the flui binary to drive (built from this checkout)')
    parser.add_argument('--idle', type=float, default=20.0, help='seconds of idle to observe')
    parser.add_argument('--bound', type=float, default=240.0,
                        help='seconds allowed for the initial build and for each rebuild')
    args = parser.parse_args()

    if sys.platform != 'darwin':
        fail('macOS only: the window oracle is the CoreGraphics window list')
    if not args.cli.exists():
        fail(f'CLI binary missing at {args.cli}; run `cargo build -p flui-cli --locked` first')
    work = args.work.resolve()
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    helper = build_helper(WINDOW_LIST_SOURCE, ROOT / 'target/hot-reload-loop/macos-window-list')
    activator = build_helper(ACTIVATE_SOURCE, ROOT / 'target/hot-reload-loop/macos-activate')

    # A generated project, exactly as a user gets it, pointed at this checkout.
    created = subprocess.run([str(args.cli), 'create', 'hr_probe', '--hot-reload',
                              f'--local={ROOT}', '--no-check', '--org', 'dev.flui',
                              '--path', str(work)], capture_output=True, text=True)
    if created.returncode:
        fail(f'flui create failed:\n{created.stdout}\n{created.stderr}')
    project = work / 'hr_probe'
    worker_src = project / 'hr_probe-logic/src/lib.rs'
    original = worker_src.read_text()
    if LABEL_LINE not in original or COUNT_LINE not in original:
        fail('the generated worker does not carry the lines the probe edits')

    # The macOS backend's occlusion/focus edges are `debug`: a reload that
    # is applied but not rebuilt because the window is hidden (frames are
    # disabled while occluded) must read as that in the log, not as a
    # missing witness.
    env = dict(os.environ, FLUI_NON_INTERACTIVE='1',
               RUST_LOG='info,flui_platform::platforms::macos=debug,flui_app::app::lifecycle=debug')
    # Apple's Python exports SDKROOT for the Command Line Tools SDK; a cargo
    # child inheriting it links against that SDK's `.tbd` stubs with the
    # selected Xcode's `ld`, which refused them outright here ("unknown
    # architecture arm64e.x1"). The generated project must build the way a
    # user's shell builds it.
    env.pop('SDKROOT', None)
    log = (work / 'run.jsonl').open('w')
    run = subprocess.Popen([str(args.cli), '--json', 'run'], cwd=project, env=env,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                           start_new_session=True)
    stream = EventStream(run, log)
    try:
        # Stage 0: build, host start, window.
        found = stream.wait_for(lambda e: e.get('event') == 'run.app.start', args.bound)
        if not found:
            fail(f'no run.app.start within {args.bound}s (see {work / "run.jsonl"})')
        cursor, start = found
        host_pid = int(start['pid'])
        window = None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and window is None:
            window = window_for_pid(helper, host_pid)
            time.sleep(0.5)
        if window is None:
            fail(f'host PID {host_pid} put no window on screen within 30s')
        print(f'STAGE0=PASS host pid={host_pid} window: {window}', flush=True)

        def edit(label_line, note):
            # The host's window must be on screen for the rebuild that follows
            # a reload to run: a hidden window has frames disabled, so a
            # reload applied while another window covers it (an operator
            # working on the same display) is rebuilt only once it is visible
            # again — correct, but not this probe's question. Bring it front
            # before every edit.
            activated = subprocess.run([str(activator), str(host_pid)], capture_output=True, text=True)
            print(f'ACTIVATE: {activated.stdout.strip()}', flush=True)
            time.sleep(0.5)
            worker_src.write_text(original.replace(LABEL_LINE, label_line).replace(COUNT_LINE, WITNESS))
            print(f'EDIT: {note}', flush=True)

        def witness(since):
            """The count the reloaded worker printed, or None if it never did."""
            found = stream.wait_for(lambda e: PROBE.search(e.get('line', '')) is not None, 30, since)
            if not found:
                occluded = [e for e in stream.snapshot()[since:]
                            if 'occlusion state changed visible=false' in e.get('line', '')]
                if occluded:
                    print('HOT_RELOAD_LOOP=CANNOT_VERIFY: the host window was occluded after the reload '
                          '(frames are disabled while hidden, so the rebuild is deferred); uncover the '
                          'window and re-run', flush=True)
                    sys.exit(2)
                return None
            return int(PROBE.search(found[1]['line']).group(1))

        def expect_reload(since, ok, stage):
            build = stream.wait_for(lambda e: e.get('event') == 'run.build.done', args.bound, since)
            if not build:
                fail(f'{stage}: no run.build.done within {args.bound}s')
            index, done = build
            if bool(done.get('ok')) != ok:
                fail(f'{stage}: run.build.done ok={done.get("ok")}, expected {ok}')
            reload = stream.wait_for(lambda e: e.get('event') == 'run.reload', 30, index)
            if not reload:
                fail(f'{stage}: no run.reload after the build')
            index, event = reload
            if event.get('kind') != 'hot' or bool(event.get('ok')) != ok:
                fail(f'{stage}: run.reload {event}, expected kind=hot ok={ok}')
            if not pid_alive(host_pid):
                fail(f'{stage}: host PID {host_pid} died')
            exited = [e for e in stream.snapshot()[since:] if e.get('event') in ('run.app.exit', 'run.app.stop')]
            if exited:
                fail(f'{stage}: the host was restarted or stopped: {exited[0]}')
            print(f'STAGE={stage} PASS: build ok={ok}, reload ok={ok}, host pid {host_pid} unchanged', flush=True)
            return index + 1

        # Stage 1: a label edit hot-reloads. The witness line did not exist
        # before this edit, so its appearance proves the reload ran the new
        # code inside the original host.
        cursor = len(stream.snapshot())
        edit('const INCREMENT_LABEL: &str = "Increment (+1) — reloaded once";',
             'label -> "reloaded once" + count witness')
        cursor = expect_reload(cursor, True, 'EDIT1')
        count = witness(cursor)
        if count is None:
            fail('EDIT1: the reloaded worker never printed its PROBE witness — the new code did not run')
        print(f'STAGE=EDIT1 witness: count={count} (new code ran in host pid {host_pid})', flush=True)

        # Stage 3: a broken edit is reported and leaves the host alone.
        cursor = len(stream.snapshot())
        edit('const INCREMENT_LABEL: &str = "Increment (+1) — broken;', 'unterminated string (syntax error)')
        cursor = expect_reload(cursor, False, 'EDIT2')
        if not pid_alive(host_pid):
            fail('EDIT2: host died after a failed worker build')

        # Stage 4: the fix reloads again, same host.
        cursor = len(stream.snapshot())
        edit('const INCREMENT_LABEL: &str = "Increment (+1) — fixed";', 'label -> "fixed"')
        cursor = expect_reload(cursor, True, 'EDIT3')
        after_fix = witness(cursor)
        if after_fix is None:
            fail('EDIT3: the fixed worker never printed its PROBE witness')
        if after_fix <= count:
            fail(f'EDIT3: state was NOT preserved: count {count} after EDIT1, {after_fix} after the fix '
                 '(a preserved counter keeps climbing; a reset one starts over)')
        print(f'STAGE=EDIT3 witness: count={after_fix} > {count} — host state preserved across two '
              'reloads and a failed rebuild', flush=True)

        # Stage 5: idle means idle.
        before = len(stream.snapshot())
        time.sleep(args.idle)
        idle_events = [e for e in stream.snapshot()[before:]
                       if e.get('event', '').startswith(('run.build', 'run.reload', 'run.change'))]
        if idle_events:
            fail(f'IDLE: {len(idle_events)} build/reload event(s) with no edit: {idle_events[0]}')
        print(f'STAGE=IDLE PASS: {args.idle:.0f}s, no build or reload event', flush=True)

        # Stage 6: Ctrl-C ends both.
        before = len(stream.snapshot())
        os.killpg(run.pid, signal.SIGINT)
        try:
            code = run.wait(timeout=30)
        except subprocess.TimeoutExpired:
            fail('SHUTDOWN: flui run did not exit within 30s of SIGINT')
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline and pid_alive(host_pid):
            time.sleep(0.2)
        if pid_alive(host_pid):
            fail(f'SHUTDOWN: host PID {host_pid} still alive 15s after flui run exited')
        names = [e.get('event') for e in stream.snapshot()[before:]]
        print(f'STAGE=SHUTDOWN PASS: flui run exit {code}, host gone, events {names}', flush=True)
        print(f'HOT_RELOAD_LOOP=PASS count_after_edit1={count} count_after_fix={after_fix}', flush=True)
    finally:
        if run.poll() is None:
            os.killpg(run.pid, signal.SIGKILL)
        log.close()


if __name__ == '__main__':
    main()
