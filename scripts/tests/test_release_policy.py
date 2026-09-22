"""Release inventory and real Cargo normalization regressions."""
import os
import json
import pathlib
import subprocess
import sys
import tarfile
import tempfile
if sys.version_info < (3, 11):  # tomllib; macOS /usr/bin/python3 is 3.9
    print(
        f"{sys.argv[0]}: needs Python >= 3.11 (it uses tomllib); running "
        f"{sys.version.split()[0]}. macOS: brew install python@3.12, then run it "
        "through just (recipes pick a Python >= 3.11). `just doctor` checks every tool.",
        file=sys.stderr,
    )
    sys.exit(2)  # same code as the shell scripts' interpreter guards
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class RepositoryPolicy(unittest.TestCase):
    def test_every_layer_member_has_an_explicit_release_role(self):
        policy = tomllib.loads((ROOT / 'docs/workspace-layers.toml').read_text())
        for member in policy['member']:
            with self.subTest(package=member['name']):
                self.assertIn(member.get('release_role'), {'facade', 'cli', 'support', 'private'})



sys.path.insert(0, str(ROOT / 'scripts'))
import release_policy


class Fixture:
    def __init__(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        (self.root / 'docs').mkdir()
        self.roles = []
        self.checkout_records = []
        self.package('flui', 'facade')
        self.package('flui-cli', 'cli')

    def package(self, name, role, extra='', version='0.2.0', publish=None):
        directory = self.root if name == 'flui' else self.root / 'crates' / name
        (directory / 'src').mkdir(parents=True, exist_ok=True)
        (directory / 'src/lib.rs').write_text('pub fn fixture() {}\n')
        for licence in release_policy.LICENCE_FILES:
            (directory / licence).write_text(f'{licence} fixture text\n')
        allowed = 'false' if role == 'private' else '["crates-io"]'
        if publish is not None:
            allowed = publish
        text = f'[package]\nname = "{name}"\nversion = "{version}"\nedition = "2024"\npublish = {allowed}\n'
        if name == 'flui':
            text += '[workspace]\nmembers = ["crates/*"]\nresolver = "3"\n'
        if name == 'flui-cli':
            text += '[[bin]]\nname = "flui"\npath = "src/main.rs"\n'
            (directory / 'src/main.rs').write_text('fn main() {}\n')
        (directory / 'Cargo.toml').write_text(text + extra)
        if role is not None:
            self.roles = [entry for entry in self.roles if entry[0] != name] + [(name, role)]
        self.policy()
        return directory

    def policy(self):
        (self.root / 'docs/workspace-layers.toml').write_text(''.join(
            f'[[member]]\nname = "{name}"\nrelease_role = "{role}"\n' for name, role in self.roles) + ''.join(self.checkout_records))

    def checkout(self, source, package, alias=None, target='all', path=None):
        alias = alias or package
        path = path or f'crates/{package}'
        record = {'source': source, 'package': package, 'alias': alias, 'target': target,
                  'path': path, 'why': 'Checkout regression fixture.'}
        self.checkout_records.append('[[checkout_only_dev]]\n' + ''.join(
            f'{key} = {json.dumps(value)}\n' for key, value in record.items()))
        self.policy()

    def append(self, name, source):
        directory = self.root if name == 'flui' else self.root / 'crates' / name
        path = directory / 'Cargo.toml'
        path.write_text(path.read_text() + source)

    def check(self):
        return release_policy.check(self.root)


class ReleaseRules(unittest.TestCase):
    def setUp(self):
        self.fixture = Fixture()
        self.addCleanup(self.fixture.temporary.cleanup)

    def errors(self):
        return '\n'.join(self.fixture.check().errors)

    def test_products_and_private_are_distinct(self):
        self.fixture.package('scratch', 'private')
        report = self.fixture.check()
        self.assertEqual(report.errors, [])
        self.assertEqual(report.selected, ['flui', 'flui-cli'])
        self.assertEqual(report.private, ['scratch'])

    def test_missing_duplicate_unknown_and_extra_product_roles(self):
        for roles, expected in [([('flui', None), ('flui-cli', 'cli')], 'unknown release_role'),
                                ([('flui', 'facade'), ('flui', 'facade')], 'duplicate'),
                                ([('flui', 'facade'), ('flui-cli', 'mystery')], 'unknown release_role'),
                                ([('flui', 'facade'), ('flui-cli', 'facade')], 'product must be exactly')]:
            with self.subTest(roles=roles):
                self.fixture.roles = roles
                self.fixture.policy()
                self.assertIn(expected, self.errors())

    def test_cli_target_and_private_restrictions(self):
        path = self.fixture.root / 'crates/flui-cli/Cargo.toml'
        path.write_text(path.read_text().replace('name = "flui"', 'name = "different"'))
        self.fixture.package('secret', 'private', publish='["crates-io"]')
        self.assertIn('binary target', self.errors())
        self.assertIn('private package must set', self.errors())

    def test_unreachable_support_and_unknown_package(self):
        self.fixture.package('unused', 'support')
        self.fixture.package('unclassified', None)
        self.assertIn('unreachable', self.errors())
        self.assertIn('unclassified publishable', self.errors())

    def test_all_retained_dependency_kinds_reject_private(self):
        self.fixture.package('secret', 'private')
        for table, option in [('dependencies', ', optional = true'),
                              ('build-dependencies', ''),
                              ("target.'cfg(target_os = \"none\")'.dependencies", ''),
                              ('dev-dependencies', '')]:
            with self.subTest(table=table):
                self.fixture.package('flui', 'facade', f'[{table}]\nsecret = {{ path = "crates/secret", version = "0.2.0"{option} }}\n')
                self.assertIn('reaches private', self.errors())

    def test_version_presence_not_metadata_wildcard_controls_dev_retention(self):
        self.fixture.package('secret', 'private')
        self.fixture.append('flui', '[dev-dependencies]\nsecret = { path = "crates/secret" }\n')
        self.fixture.checkout('flui', 'secret')
        self.assertEqual(self.fixture.check().errors, [])
        self.fixture.package('flui', 'facade', '[workspace.dependencies]\nsecret = { path = "crates/secret", version = "*" }\n[dev-dependencies]\nsecret.workspace = true\n')
        self.assertIn('non-wildcard', self.errors())
        self.assertIn('reaches private', self.errors())

    def test_inherited_alias_uses_workspace_path_and_package_identity(self):
        self.fixture.package('support', 'support')
        self.fixture.append('flui', '[workspace.dependencies]\nalias = { package = "support", path = "crates/support", version = "0.2.0" }\n')
        self.fixture.append('flui-cli', '[dependencies]\nalias.workspace = true\n')
        self.assertEqual(self.fixture.check().errors, [])
        self.assertEqual(self.fixture.check().reasons['support'], ['flui-cli', 'support'])

    def test_path_identity_and_unknown_internal_are_checked(self):
        self.fixture.package('support', 'support')
        self.fixture.append('flui', '[dependencies]\nwrong = { path = "crates/support", version = "0.2.0" }\nflui-unknown = "0.2.0"\nmissing = { path = "crates/missing", version = "0.2.0" }\n')
        self.assertIn('package/path mismatch', self.errors())
        self.assertIn('unknown internal package', self.errors())
        self.assertIn('unknown internal dependency path', self.errors())

    def test_external_sources_are_checked_even_for_foreign_targets(self):
        self.fixture.append('flui', '[target.\'cfg(target_os = "none")\'.build-dependencies]\nexternal = { git = "https://invalid.example/repo" }\nother = { version = "1", registry = "private" }\n')
        self.assertIn('non-wildcard', self.errors())
        self.assertIn('must resolve from crates.io', self.errors())

    def test_git_and_version_is_a_valid_registry_fallback(self):
        self.fixture.append('flui', '[dependencies]\nexternal = { git = "https://invalid.example/repo", version = "1" }\n')
        self.assertEqual(self.fixture.check().errors, [])

    def test_prerelease_cohort_requires_exact_version(self):
        self.fixture.package('support', 'support', version='0.2.0-beta.1')
        for requirement, valid in [('0.2.0', False), ('0.2.0-beta.1', False), ('=0.2.0-beta.2', False), ('=0.2.0-beta.1', True)]:
            with self.subTest(requirement=requirement):
                self.fixture.package('flui', 'facade', f'[dependencies]\nsupport = {{ path = "crates/support", version = "{requirement}" }}\n')
                self.assertEqual(not self.fixture.check().errors, valid)

    def test_build_cycle_is_rejected_but_dev_cycle_is_not(self):
        self.fixture.package('support', 'support', '[dev-dependencies]\nflui = { path = "../..", version = "0.2.0" }\n')
        self.fixture.append('flui', '[dependencies]\nsupport = { path = "crates/support", version = "0.2.0" }\n')
        self.assertEqual(self.fixture.check().errors, [])
        self.assertEqual(self.fixture.check().dependency_cycles[0]['packages'], ['flui', 'support'])
        self.assertEqual({edge['kind'] for edge in self.fixture.check().dependency_cycles[0]['edges']}, {'normal', 'dev'})
        path = self.fixture.root / 'crates/support/Cargo.toml'
        path.write_text(path.read_text().replace('[dev-dependencies]', '[build-dependencies]'))
        self.assertIn('normal/build dependency cycle', self.errors())

    def test_unrecorded_checkout_only_dev_edge_is_rejected(self):
        self.fixture.package('secret', 'private')
        self.fixture.append('flui', '[dev-dependencies]\nsecret = { path = "crates/secret" }\n')
        self.assertIn('unrecorded checkout-only dev dependency', self.errors())

    def test_inherited_local_features_and_flags_are_preserved(self):
        document = {'dependencies': {'alias': {'workspace': True, 'features': ['local'], 'optional': True, 'default-features': True}}}
        workspace = {'dependencies': {'alias': {'package': 'support', 'path': 'crates/support', 'version': '0.2.0', 'features': ['base'], 'default-features': False}}}
        _, spec, _, _, _ = next(release_policy.declarations(document, self.fixture.root, self.fixture.root, workspace))
        self.assertEqual(set(spec['features']), {'base', 'local'})
        self.assertTrue(spec['optional'])
        self.assertTrue(spec['default-features'])

    def test_checkout_records_reject_duplicate_stale_and_wrong_identity(self):
        f = self.fixture
        f.package('secret', 'private')
        f.append('flui', '[dev-dependencies]\nrenamed = { package = "secret", path = "crates/secret" }\n')
        f.checkout('flui', 'secret', alias='renamed')
        self.assertEqual(f.check().errors, [])
        f.checkout('flui', 'secret', alias='renamed')
        self.assertIn('duplicate checkout-only', self.errors())
        f.checkout_records.clear()
        f.checkout('flui', 'secret', alias='wrong')
        self.assertIn('stale checkout-only', self.errors())
        self.assertIn('unrecorded checkout-only', self.errors())
        path = f.root / 'Cargo.toml'
        path.write_text(path.read_text().replace('path = "crates/secret"', 'path = "crates/missing"'))
        self.assertIn('unknown internal dependency path', self.errors())

    def test_checkout_record_target_and_alias_are_exact(self):
        f = self.fixture
        f.package('secret', 'private')
        target = 'cfg(target_os = "none")'
        f.append('flui', f"[target.'{target}'.dev-dependencies]\nrenamed = {{ package = \"secret\", path = \"crates/secret\" }}\n")
        f.checkout('flui', 'secret', alias='renamed', target=target)
        self.assertEqual(f.check().errors, [])
        f.checkout_records.clear()
        f.checkout('flui', 'secret', alias='renamed')
        self.assertIn('stale checkout-only', self.errors())
        self.assertIn('unrecorded checkout-only', self.errors())

    def test_versioned_declaration_cannot_keep_a_checkout_record(self):
        f = self.fixture
        f.package('support', 'support')
        f.append('flui', '[dev-dependencies]\nsupport = { path = "crates/support", version = "0.2.0" }\n')
        f.checkout('flui', 'support')
        self.assertIn('stale checkout-only', self.errors())

    def test_focused_cut_retains_lockfiles_and_local_features(self):
        f = self.fixture
        f.package('support-a', 'support', '[features]\nbase = []\nlocal = []\n[dev-dependencies]\nsupport-b = { path = "../support-b", version = "0.2.0" }\n')
        f.package('support-b', 'support', '[dev-dependencies]\nsupport-a = { path = "../support-a", features = ["local"] }\n')
        f.checkout('support-b', 'support-a')
        f.append('flui', '[workspace.dependencies]\nalias = { package = "support-a", path = "crates/support-a", version = "0.2.0", default-features = false, features = ["base"] }\n[dependencies]\nalias = { workspace = true, features = ["local"], optional = true, default-features = true }\n')
        f.append('flui-cli', '[dependencies]\nalias = { workspace = true, features = ["local"] }\n')
        report = f.check()
        self.assertEqual(report.errors, [])
        self.assertEqual(report.dependency_cycles, [])
        env = {**os.environ, 'CARGO_HOME': str(f.root / 'isolated-cargo-home')}
        metadata = subprocess.run(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=f.root, env=env, text=True, capture_output=True)
        self.assertEqual(metadata.returncode, 0, metadata.stderr)
        packages = {p['name']: p for p in json.loads(metadata.stdout)['packages']}
        local = packages['support-b']['dependencies'][0]
        self.assertEqual(local['features'], ['local'])
        self.assertEqual(local['path'], str((f.root / 'crates/support-a').resolve()))
        args = release_policy.package_arguments(report.selected) + ['--offline', '--target-dir', str(f.root / 'target/release-policy')]
        result = subprocess.run(args, cwd=f.root, env=env, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(release_policy.verify_archives(f.root, report.selected), [])
        for name in report.selected:
            with tarfile.open(f.root / f'target/release-policy/package/{name}-0.2.0.crate') as archive:
                self.assertIn(f'{name}-0.2.0/Cargo.lock', archive.getnames())
                document = tomllib.loads(archive.extractfile(f'{name}-0.2.0/Cargo.toml').read().decode())
            if name == 'support-b':
                self.assertNotIn('support-a', document.get('dev-dependencies', {}))
            if name == 'support-a':
                self.assertIn('support-b', document['dev-dependencies'])
            if name == 'flui':
                self.assertEqual(set(document['dependencies']['alias']['features']), {'base', 'local'})
                self.assertTrue(document['dependencies']['alias']['optional'])
                self.assertTrue(document['dependencies']['alias']['default-features'])
            if name == 'flui-cli':
                self.assertFalse(document['dependencies']['alias']['default-features'])

    def test_package_arguments_are_explicit_and_dirty_is_opt_in(self):
        args = release_policy.package_arguments(['flui', 'flui-cli'])
        self.assertNotIn('--workspace', args)
        self.assertNotIn('--allow-dirty', args)
        self.assertEqual(args.count('-p'), 2)
        self.assertIn('--allow-dirty', release_policy.package_arguments(['flui'], True))

    def test_real_cargo_archives_preserve_normalized_closure(self):
        f = self.fixture
        f.package('support-a', 'support', '[dev-dependencies]\nsupport-b = { path = "../support-b", version = "0.2.0" }\n')
        f.package('support-b', 'support')
        f.package('secret', 'private')
        f.append('flui', '[dependencies]\nalias = { package = "support-a", path = "crates/support-a", version = "0.2.0", optional = true }\n[build-dependencies]\nsupport-b = { path = "crates/support-b", version = "0.2.0" }\n[dev-dependencies]\nsecret = { path = "crates/secret" }\n[target.\'cfg(target_os = "none")\'.dependencies]\nsupport-b = { path = "crates/support-b", version = "0.2.0" }\n')
        f.checkout('flui', 'secret')
        report = f.check()
        self.assertEqual(report.errors, [])
        args = release_policy.package_arguments(report.selected)
        args += ['--offline', '--target-dir', str(f.root / 'target/release-policy')]
        output = subprocess.run(args, cwd=f.root, text=True, capture_output=True,
                                env={**os.environ, "CARGO_HOME": str(f.root / "isolated-cargo-home")})
        self.assertEqual(output.returncode, 0, output.stdout + output.stderr)
        self.assertEqual(release_policy.verify_archives(f.root, report.selected), [])
        archive_root = f.root / 'target/release-policy/package'
        with tarfile.open(archive_root / 'flui-0.2.0.crate') as archive:
            document = tomllib.loads(archive.extractfile('flui-0.2.0/Cargo.toml').read().decode())
        self.assertNotIn('secret', document.get('dev-dependencies', {}))
        self.assertEqual(document['dependencies']['alias']['package'], 'support-a')
        self.assertTrue(document['dependencies']['alias']['optional'])
        self.assertIn('support-b', document['build-dependencies'])
        self.assertIn('support-b', document['target']['cfg(target_os = "none")']['dependencies'])
        for name, other in [('support-a', 'support-b')]:
            with tarfile.open(archive_root / f'{name}-0.2.0.crate') as archive:
                document = tomllib.loads(archive.extractfile(f'{name}-0.2.0/Cargo.toml').read().decode())
            self.assertIn(other, document['dev-dependencies'])

    def test_fresh_versioned_dev_cycle_is_legal_but_cargo_reports_archive_blocker(self):
        f = self.fixture
        f.package('support-a', 'support', '[dev-dependencies]\nsupport-b = { path = "../support-b", version = "0.2.0" }\n')
        f.package('support-b', 'support', '[dev-dependencies]\nsupport-a = { path = "../support-a", version = "0.2.0" }\n')
        f.append('flui', '[dependencies]\nsupport-a = { path = "crates/support-a", version = "0.2.0" }\n')
        report = f.check()
        self.assertEqual(report.errors, [])
        output = subprocess.run(release_policy.package_arguments(report.selected) + ['--offline'],
                                cwd=f.root, text=True, capture_output=True,
                                env={**os.environ, "CARGO_HOME": str(f.root / "isolated-cargo-home")})
        self.assertNotEqual(output.returncode, 0)
        self.assertIn('no matching package named', output.stderr)
        self.assertIn('support-', output.stderr)


if __name__ == '__main__':
    unittest.main()
