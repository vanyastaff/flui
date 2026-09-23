"""scripts/lib/change_scope.py and cargo_args.py: how a change is scoped.

Path classification needs no git or cargo; the dependency-graph cases run a
real `cargo metadata --no-deps --offline`; the rename case builds a throwaway
git repository. Runs on a stock python3 >= 3.9.
"""
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))
import cargo_args as ca  # noqa: E402
import change_scope as cs  # noqa: E402


def scope(*files):
    return cs.classify(list(files))


class DocsOnly(unittest.TestCase):
    def test_documentation_paths(self):
        for path in ("README.md", "docs/testing.md", "book/src/intro.md", ".rust-studio/specs/x.md",
                     ".github/PULL_REQUEST_TEMPLATE.md", "crates/flui-view/ARCHITECTURE.md",
                     "crates/flui-view/CHANGELOG.md"):
            self.assertTrue(cs.is_docs_only(path), path)

    def test_compiled_markdown_is_not_docs(self):
        # crate READMEs are include_str!()'d into doctests; a nested .md is not a root .md
        for path in ("crates/flui-animation/README.md", "crates/flui-cli/templates/platforms/ios/README.md",
                     ".github/workflows/ci.yml", "src/lib.rs"):
            self.assertFalse(cs.is_docs_only(path), path)


class Modes(unittest.TestCase):
    def test_docs_and_empty(self):
        self.assertEqual(scope("docs/a.md", "README.md")["mode"], "docs")
        self.assertEqual(scope()["mode"], "docs")

    def test_heavy_inputs_require_the_heavy_lane(self):
        for path in ("Cargo.lock", "Cargo.toml", ".cargo/config.toml", "rust-toolchain.toml",
                     "rust-toolchain", ".github/workflows/ci.yml"):
            r = scope(path)
            self.assertEqual((r["mode"], r["heavy_required"]), ("full", True), path)

    def test_scripts_only_a_heavy_job_runs_require_the_heavy_lane(self):
        # read out of ci.yml, not restated: doc-strict (doc job), the wasm-check helpers
        inputs = cs.heavy_job_inputs()
        for path in ("scripts/doc-strict.sh", "scripts/check-wasm-imports.sh", "scripts/wasm-test-crates.py"):
            self.assertIn(path, inputs)
            self.assertTrue(scope(path)["heavy_required"], path)

    def test_lane_machinery_gets_the_whole_workspace(self):
        for path in ("scripts/lib/change_scope.py", "scripts/lib/cargo_args.py", "scripts/affected-crates.sh",
                     "scripts/lib/interpreters.sh", "clippy.toml", ".config/nextest.toml"):
            r = scope(path)
            self.assertEqual((r["mode"], r["heavy_required"]), ("full", False), path)

    def test_checks_only_tooling_compiles_nothing(self):
        self.assertEqual(scope("scripts/port-check.sh", "typos.toml")["mode"], "none")

    def test_unattributable_file_is_full(self):
        self.assertEqual(scope("some-new-dir/thing.txt")["mode"], "full")


class Graph(unittest.TestCase):
    def test_crate_change_pulls_in_its_dependents(self):
        r = scope("crates/flui-material/src/lib.rs")
        self.assertEqual(r["mode"], "packages")
        self.assertIn("flui", r["packages"])  # the facade depends on it
        self.assertNotIn("flui-types", r["packages"])  # a dependency, not a dependent

    def test_optional_dependency_edges_count(self):
        # declared but feature-gated: the resolved graph would miss these
        self.assertIn("flui", scope("crates/flui-cupertino/src/lib.rs")["packages"])
        self.assertIn("flui", scope("crates/flui-localizations/src/lib.rs")["packages"])
        self.assertIn("flui-widgets", scope("crates/flui-assets/src/lib.rs")["packages"])

    def test_root_package_owns_its_targets_directories(self):
        r = scope("examples/material_demo/tree.rs")  # an [[example]] whose main.rs is in a subdirectory
        self.assertEqual(r["mode"], "packages")
        self.assertIn("flui", r["packages"])
        r = scope("examples/web_counter/src/lib.rs")  # a separate package under examples/
        self.assertEqual((r["mode"], r["packages"]), ("packages", ["flui-web-counter"]))

    def test_a_changed_manifest_is_reported(self):
        r = scope("crates/flui-material/Cargo.toml")
        self.assertEqual(r["manifests"], ["flui-material"])
        self.assertEqual(ca.args_for(r)["hack_args"], "-p flui-material")


class Renames(unittest.TestCase):
    def test_a_move_puts_both_crates_in_scope(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            git = lambda *a: subprocess.run(["git", *a], cwd=root, check=True, capture_output=True)  # noqa: E731
            git("init", "-q", "-b", "main")
            git("config", "user.email", "t@example.invalid")
            git("config", "user.name", "t")
            (root / "a").mkdir()
            (root / "a" / "x.rs").write_text("fn x() {}\n" * 20)
            git("add", ".")
            git("commit", "-qm", "base")
            git("checkout", "-qb", "change")
            (root / "b").mkdir()
            git("mv", "a/x.rs", "b/x.rs")
            git("commit", "-qm", "move")
            self.assertEqual(cs.changed_files("main", False, root=root), ["a/x.rs", "b/x.rs"])


class CargoArgs(unittest.TestCase):
    def test_cfg_gated_backends_get_their_targets(self):
        a = ca.args_for(scope("crates/flui-platform/src/lib.rs"))
        self.assertEqual((a["cross_platform"], a["cross_app"], a["platform"]), ("true", "true", "true"))
        self.assertNotIn("flui-platform", a["test_args"])  # its suite runs in the headless leg
        a = ca.args_for(scope("crates/flui-material/src/lib.rs"))
        self.assertEqual((a["cross_platform"], a["cross_cli"]), ("false", "false"))
        self.assertEqual(a["cross_app"], "true")  # `flui` is in scope: its mobile runner is

    def test_wasm_scope_skips_packages_that_cannot_target_wasm(self):
        no_wasm = ca.no_wasm_packages()
        self.assertIn("flui-cli", no_wasm)  # read from ci.yml, not restated
        a = ca.args_for(scope("crates/flui-platform/src/lib.rs"))
        self.assertNotIn("-p flui-cli", a["wasm_args"])
        self.assertIn("-p flui-platform", a["wasm_args"])

    def test_per_feature_pass_covers_the_changed_crates_features(self):
        # a source-only change to a crate with non-default features gets the
        # per-feature clippy; its dependents do not (feature-matrix covers them)
        a = ca.args_for(scope("crates/flui-assets/src/lib.rs"))
        self.assertEqual(a["hack_args"], "-p flui-assets")
        self.assertNotIn("flui-widgets", a["hack_args"])

    def test_ios_leg_when_flui_app_is_in_scope(self):
        self.assertEqual(ca.args_for(scope("crates/flui-view/src/lib.rs"))["cross_ios"], "true")
        self.assertEqual(ca.args_for(scope("crates/flui-material/src/lib.rs"))["cross_ios"], "false")

    def test_rustdoc_covers_the_scope_with_its_testing_features(self):
        a = ca.args_for(scope("crates/flui-material/src/lib.rs"))
        self.assertTrue(a["doc_args"].startswith("-p flui -p flui-material"))
        self.assertIn("--features flui/testing", a["doc_args"])
        # a testing feature of a package outside the scope would be rejected by cargo
        self.assertNotIn("flui-rendering/testing", a["doc_args"])
        self.assertEqual(ca.args_for(scope("docs/x.md"))["doc_args"], "")

    def test_shell_format_is_safe_to_eval(self):
        # `just check-changed` evals the CLI's --format shell output; an unowned
        # file's name lands in `reason`, so a hostile file name must stay data.
        payload = "x';touch${IFS}PWNED;#"
        cli = Path(ca.__file__)
        out = subprocess.run([sys.executable, str(cli), "--files", payload, "--format", "shell"],
                             check=True, capture_output=True, text=True).stdout
        with tempfile.TemporaryDirectory() as tmp:
            subprocess.run(["bash", "-c", 'eval "$1"; printf %s "$REASON"', "_", out],
                           cwd=tmp, check=True, capture_output=True)
            self.assertFalse(os.path.exists(os.path.join(tmp, "PWNED")))


if __name__ == "__main__":
    unittest.main()
