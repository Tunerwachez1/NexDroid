#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import json
import pathlib
import runpy
import tempfile
import unittest

verify = runpy.run_path(str(pathlib.Path(__file__).resolve().parents[1] /
    'dist/standalone/verify-payload.py'))['verify']

class PayloadTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        (self.root / 'program').write_bytes(b'payload')
        (self.root / 'program').chmod(0o755)
        (self.root / 'alias').symlink_to('program')
        self.entries = {'program': {'sha256': hashlib.sha256(b'payload').hexdigest(), 'exec': 0o111},
                        'alias': {'symlink': 'program'}}
        self.manifest()

    def manifest(self):
        data = json.dumps({'format': 1, 'files': self.entries}).encode()
        (self.root / 'payload.json').write_bytes(data)
        (self.root / 'payload-id').write_text(hashlib.sha256(data).hexdigest())

    def test_intact_package(self):
        verify(self.root)

    def test_changed_binary_is_rejected(self):
        (self.root / 'program').write_bytes(b'tampered')
        with self.assertRaises(AssertionError): verify(self.root)

    def test_unlisted_executable_is_rejected(self):
        (self.root / 'extra').write_text('unexpected')
        with self.assertRaises(AssertionError): verify(self.root)

    def test_changed_symlink_is_rejected(self):
        (self.root / 'alias').unlink()
        (self.root / 'alias').symlink_to('/etc/passwd')
        with self.assertRaises(AssertionError): verify(self.root)

    def test_exec_mode_is_checked(self):
        (self.root / 'program').chmod(0o644)
        with self.assertRaises(AssertionError): verify(self.root)

    def test_parent_traversal_is_rejected(self):
        self.entries['../outside'] = self.entries.pop('program')
        self.manifest()
        with self.assertRaises(AssertionError): verify(self.root)

if __name__ == '__main__':
    unittest.main()
