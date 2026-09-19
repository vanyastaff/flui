"""Offline asset-policy mutations and deterministic fixture controls."""
import hashlib
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import font_assets


class FontAssets(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        shutil.copytree(ROOT / font_assets.ASSETS, self.root / font_assets.ASSETS)
        shutil.copytree(ROOT / 'tools/decoy-face', self.root / 'tools/decoy-face')
        self.assets = self.root / font_assets.ASSETS

    def test_inventory_and_every_generated_file_reproduce_offline(self):
        self.assertEqual(font_assets.check(self.root), [])
        self.assertEqual(font_assets.reproduce(self.root), [])

    def test_existing_four_generated_fixtures_are_byte_unchanged(self):
        # Historical controls: extending coverage must not alter other probes.
        expected = {
            'decoy-wide-space.ttf': 'a6fc5cabe4943b7ee032651bd7d7c0bef614f0b5a40bd1dfab62db5e516e0888',
            'probe-mono-100.ttf': '8b8b0fd14500faba32b7b9e1bdd1dedca42c89c0bd43451ae183b8cf391e1221',
            'probe-mono-600.ttf': 'c06729a30262f8b541d33f9c8744885f937cb688834b68fb8f1e0b8b1651f996',
            'probe-variable-wght.ttf': '9e3e800de59a6b850039e240192dce3ebecbd6625f222e57e22d1abc2b57acb5',
        }
        for name, digest in expected.items():
            with self.subTest(font=name):
                self.assertEqual(hashlib.sha256((self.assets / name).read_bytes()).hexdigest(), digest)

    def test_changed_font_bytes_fail(self):
        (self.assets / 'Roboto-Regular.ttf').write_bytes(b'changed')
        self.assertTrue(any('font hash mismatch' in error for error in font_assets.check(self.root)))

    def test_missing_and_modified_license_fail(self):
        notice = self.assets / 'licenses/Roboto-Apache-2.0.txt'
        notice.write_text('not the complete license')
        self.assertTrue(any('notice hash mismatch' in error for error in font_assets.check(self.root)))
        notice.unlink()
        self.assertTrue(any('missing license/notice' in error for error in font_assets.check(self.root)))

    def test_unlisted_font_fails_inside_and_outside_current_owner(self):
        (self.assets / 'surprise.otf').write_bytes(b'font')
        elsewhere = self.root / 'crates/new-owner/assets'
        elsewhere.mkdir(parents=True)
        (elsewhere / 'surprise.woff2').write_bytes(b'font')
        errors = '\n'.join(font_assets.check(self.root))
        self.assertIn('unlisted font asset', errors)
        self.assertIn('unlisted font outside', errors)

    def test_reintroduced_arial_name_is_rejected(self):
        shutil.copyfile(self.assets / 'probe-sans-400.ttf', self.assets / 'Arial.ttf')
        self.assertTrue(any('restricted Arial' in error for error in font_assets.check(self.root)))

    def test_missing_provenance_and_notice_association_fail(self):
        inventory = self.assets / 'inventory.toml'
        source = inventory.read_text()
        source = '\n'.join(line for line in source.splitlines() if not line.startswith(('source =', 'notices =')))
        inventory.write_text(source)
        errors = '\n'.join(font_assets.check(self.root))
        self.assertIn('missing source or license provenance', errors)
        self.assertIn('missing recorded license/notice association', errors)

    def test_modified_generator_is_detected_without_overwriting_assets(self):
        generator = self.root / 'tools/decoy-face/generate.py'
        generator.write_text(generator.read_text().replace('space_advance=500', 'space_advance=501'))
        before = (self.assets / 'probe-sans-400.ttf').read_bytes()
        self.assertTrue(any('not reproducible' in error for error in font_assets.reproduce(self.root)))
        self.assertEqual((self.assets / 'probe-sans-400.ttf').read_bytes(), before)


if __name__ == '__main__':
    unittest.main()
