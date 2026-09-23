"""The `ci` aggregator's skip rule, exercised on synthetic job results.

The aggregator (ci.yml, job `ci`) decides from `plan`'s outputs which jobs a
run may skip and fails on anything else. A regression there either lets a
heavy job silently skip (red main that looks green) or fails every PR. This
extracts the aggregator's own Python from ci.yml -- no copy -- and runs it
against each lane/mode with a correct and a drifted set of job results.

Needs PyYAML (the aggregator itself does; CI installs it).
"""
import json
import os
import re
import subprocess
import sys
import unittest
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover - a skip locally, an error on CI
    if os.environ.get("CI"):
        raise  # CI installs PyYAML for this; a silent skip there would hide a broken aggregator
    yaml = None

ROOT = Path(__file__).resolve().parent.parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"


@unittest.skipIf(yaml is None, "PyYAML not installed (pip install 'pyyaml>=6,<7')")
class AggregatorSkipRule(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
        cls.jobs = workflow["jobs"]
        step = next(s for s in cls.jobs["ci"]["steps"] if s.get("name", "").startswith("Verify"))
        cls.code = re.search(r"python3 - <<'EOF'\n(.*)\nEOF", step["run"], re.S).group(1)
        cls.heavy_jobs = step["env"]["HEAVY_JOBS"]
        cls.gated = [j for j in cls.jobs if j not in ("ci", "notify-main-red")]

    def aggregate(self, heavy, mode, results, event="pull_request"):
        needs = {j: {"result": results.get(j, "success")} for j in self.gated}
        env = dict(os.environ, NEEDS=json.dumps(needs), HEAVY_JOBS=self.heavy_jobs,
                   HEAVY=heavy, MODE=mode, EVENT=event)
        run = subprocess.run([sys.executable, "-c", self.code], env=env, cwd=ROOT,
                             capture_output=True, text=True)
        return run.returncode, run.stdout + run.stderr

    def skipped(self, *extra):
        return {**{h: "skipped" for h in self.heavy_jobs.split()}, **{j: "skipped" for j in extra}}

    def assertGreen(self, *args, **kw):
        rc, out = self.aggregate(*args, **kw)
        self.assertEqual(rc, 0, out)

    def assertRed(self, *args, expect=None, **kw):
        rc, out = self.aggregate(*args, **kw)
        self.assertNotEqual(rc, 0, out)
        if expect:
            self.assertIn(expect, out)

    def test_heavy_jobs_list_matches_the_jobs_gated_on_heavy(self):
        gated_on_heavy = {j for j, d in self.jobs.items() if d.get("if") == "needs.plan.outputs.heavy == 'true'"}
        self.assertEqual(gated_on_heavy, set(self.heavy_jobs.split()))

    def test_heavy_lane(self):
        self.assertGreen("true", "full", {"fast-lane": "skipped"}, event="push")
        self.assertRed("true", "full", {"fast-lane": "skipped", "miri": "skipped"}, event="push", expect="miri")
        self.assertRed("true", "full", {"fast-lane": "skipped", "test": "failure"}, event="push", expect="test")
        self.assertRed("true", "full", {}, event="push", expect="fast-lane")

    def test_fast_lane_packages_and_full(self):
        for mode in ("packages", "full"):
            self.assertGreen("false", mode, self.skipped())
        self.assertRed("false", "packages", {**self.skipped(), "fast-lane": "failure"}, expect="fast-lane")
        ran_anyway = {k: v for k, v in self.skipped().items() if k != "doc"}
        self.assertRed("false", "packages", ran_anyway, expect="doc")

    def test_fast_lane_tooling_and_docs(self):
        self.assertGreen("false", "none", self.skipped("fast-lane"))
        self.assertGreen("false", "docs", self.skipped("fast-lane", "deny"))
        self.assertRed("false", "docs", self.skipped("fast-lane"), expect="deny")

    def test_failed_plan_is_red(self):
        self.assertRed("", "", {"plan": "failure"}, expect="no usable plan")


if __name__ == "__main__":
    unittest.main()
