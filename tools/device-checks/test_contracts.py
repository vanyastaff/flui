"""Portable driver contracts; native macOS window/signal checks run separately."""

import importlib.util
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


launch = load('launch', 'tools/device-checks/check-macos-launch-render.py')
hot = load('hot', 'tools/device-checks/check-hot-reload-loop.py')


class Contracts(unittest.TestCase):
    def test_nonpositive_launch_count_is_argument_error(self):
        for count in (0, -1):
            with self.subTest(count=count):
                result = subprocess.run([sys.executable, str(ROOT / 'tools/device-checks/check-macos-launch-render.py'), 'absent.app', '--runs', str(count)], capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 2)
                self.assertIn('--runs must be greater than zero', result.stderr)
                self.assertNotIn('PASS', result.stdout)

    def test_bmp_chrome_is_excluded_in_both_storage_orientations(self):
        width, height = 200, 200
        header = bytearray(54)
        header[:2] = b'BM'
        struct.pack_into('<I', header, 10, 54)
        struct.pack_into('<i', header, 18, width)
        struct.pack_into('<H', header, 28, 24)
        # All native content is black. Only top chrome has colored bands.
        rows = [bytes(((0, 0, 0) if y >= 26 else ((y // 3) * 24, 48, 240))) * width for y in range(height)]
        with tempfile.TemporaryDirectory() as directory:
            histograms = []
            for signed_height, stored in ((height, reversed(rows)), (-height, rows)):
                struct.pack_into('<i', header, 22, signed_height)
                file = Path(directory) / f'{signed_height}.bmp'
                file.write_bytes(header + b''.join(stored))
                w, h, counts = launch.content_histogram(file)
                self.assertEqual((w, h), (width, height))
                self.assertEqual(set(counts), {(0, 0, 0)})
                self.assertIsNotNone(launch.judge(counts, None))
                histograms.append(counts)
            self.assertEqual(histograms[0], histograms[1])

    def test_shutdown_requires_public_interrupt_contract(self):
        good = [{'event': 'run.stop', 'interrupted': True}, {'event': 'error', 'code': 130}]
        self.assertIsNone(hot.shutdown_error(130, good))
        for code, events in ((-11, good), (0, good), (130, []), (130, good[:1]), (130, good + [{'event': 'error', 'code': 1}]), (130, [{'event': 'run.stop', 'interrupted': False}, good[1]])):
            with self.subTest(code=code, events=events):
                self.assertIsNotNone(hot.shutdown_error(code, events))

    def test_dead_cli_still_retires_group_and_drains_log(self):
        with tempfile.TemporaryDirectory() as directory:
            log = (Path(directory) / 'events.log').open('w')
            child = subprocess.Popen([sys.executable, '-c', 'print(\'{"event":"error","code":130}\')'], stdout=subprocess.PIPE)
            stream = hot.EventStream(child, log)
            child.wait(timeout=10)
            retired = []
            # SIGKILL/killpg are native POSIX APIs; this test checks ownership
            # after an actually exited child, not native group signal delivery.
            with patch.object(hot.os, 'killpg', lambda pid, sig: retired.append(pid), create=True), patch.object(hot.signal, 'SIGKILL', 9, create=True):
                self.assertEqual(hot.cleanup_run(child, stream, log), [])
            self.assertEqual(retired, [child.pid])
            self.assertFalse(stream.thread.is_alive())
            self.assertTrue(log.closed)
            self.assertEqual(stream.snapshot(), [{'event': 'error', 'code': 130}])
            child.stdout.close()


if __name__ == '__main__':
    unittest.main(verbosity=2)
