"""scripts/lib/change_scope.py: how a changed path is classified.

Pure path classification (no git, no cargo) plus one real `cargo metadata`
closure check. Runs on a stock python3 >= 3.9.
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "lib"))
import change_scope as cs  # noqa: E402


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
        self.assertEqual(cs.classify(["docs/a.md", "README.md"])[0], "docs")
        self.assertEqual(cs.classify([])[0], "docs")

    def test_workspace_wide_inputs_are_full(self):
        for path in ("Cargo.lock", "Cargo.toml", ".cargo/config.toml", "rust-toolchain.toml",
                     ".github/workflows/ci.yml", ".config/nextest.toml"):
            self.assertEqual(cs.classify([path])[0], "full", path)

    def test_tooling_compiles_nothing(self):
        self.assertEqual(cs.classify(["justfile", "scripts/port-check.sh", "typos.toml"])[0], "none")

    def test_unattributable_file_is_full(self):
        self.assertEqual(cs.classify(["some-new-dir/thing.txt"])[0], "full")

    def test_crate_change_pulls_in_its_dependents(self):
        mode, packages, _ = cs.classify(["crates/flui-material/src/lib.rs"])
        self.assertEqual(mode, "packages")
        self.assertIn("flui-material", packages)
        self.assertIn("flui", packages)  # the facade depends on it
        self.assertNotIn("flui-types", packages)  # a dependency, not a dependent

    def test_root_package_owns_only_its_own_sources(self):
        self.assertEqual(cs.classify(["examples/counter.rs"])[1][:1], ["flui"])
        mode, packages, _ = cs.classify(["examples/web_counter/src/lib.rs"])
        self.assertEqual((mode, packages), ("packages", ["flui-web-counter"]))


if __name__ == "__main__":
    unittest.main()
