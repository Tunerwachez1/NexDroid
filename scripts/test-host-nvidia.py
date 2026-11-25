#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Host/runtime separation and trust boundary regressions (no root/GPU)."""
import json
import os
from pathlib import Path
import runpy
import stat
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]

class HostCacheTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / 'host-driver.json'
        self.data = {'format': 1, 'driver': '777.11.22', 'cache': [
            [0x303, 'libEGL_nvidia.so.0', '/usr/lib/libEGL_nvidia.so.777.11.22'],
            [0x003, 'libnvidia-eglcore.so.777.11.22', '/usr/lib32/libnvidia-eglcore.so.777.11.22']]}
        self.ns = runpy.run_path(str(ROOT / 'dist/standalone/host-cache-adapter.py'),
            init_globals={'os': os, 'read_ld_cache': lambda path=None: [('original', path)],
                          'driver_version': lambda: '777.11.22'})
        self.env = {'NEXDROID_HOST_DRIVER_MANIFEST': str(self.path),
                    'NEXDROID_HOST_DRIVER_ROOT': '/private/runtime/host'}

    def read(self, uid=0, mode=stat.S_IFREG | 0o644):
        self.path.write_text(json.dumps(self.data))
        with patch.dict(os.environ, self.env), patch('os.stat', return_value=SimpleNamespace(st_uid=uid, st_mode=mode)):
            return self.ns['read_ld_cache']()

    def test_future_version_uses_actual_host_files_for_both_abis(self):
        entries = self.read()
        self.assertEqual(entries[0][2], '/private/runtime/host/usr/lib/libEGL_nvidia.so.777.11.22')
        self.assertEqual(entries[1][2], '/private/runtime/host/usr/lib32/libnvidia-eglcore.so.777.11.22')

    def test_kernel_userspace_version_mismatch_is_rejected(self):
        self.data['driver'] = '777.11.21'
        with self.assertRaisesRegex(OSError, 'driver changed'): self.read()

    def test_user_owned_manifest_is_rejected(self):
        with self.assertRaisesRegex(OSError, 'unsafe'): self.read(uid=1000)

    def test_writable_manifest_is_rejected(self):
        with self.assertRaisesRegex(OSError, 'unsafe'): self.read(mode=stat.S_IFREG | 0o666)

    def test_symlink_manifest_is_rejected(self):
        with self.assertRaisesRegex(OSError, 'unsafe'): self.read(mode=stat.S_IFLNK | 0o777)

    def test_parent_traversal_is_rejected(self):
        self.data['cache'][0][2] = '/usr/../../tmp/injected.so'
        with self.assertRaisesRegex(OSError, 'unsupported'): self.read()

    def test_unmirrored_driver_path_is_rejected(self):
        self.data['cache'][0][2] = '/tmp/libEGL_nvidia.so.777.11.22'
        with self.assertRaisesRegex(OSError, 'unsupported'): self.read()

    def test_explicit_cache_uses_upstream_reader(self):
        self.assertEqual(self.ns['read_ld_cache']('/explicit/cache'), [('original', '/explicit/cache')])

    def test_outside_runtime_uses_upstream_reader(self):
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(self.ns['read_ld_cache'](), [('original', None)])

class LinuxLookupTests(unittest.TestCase):
    def test_preflight_unpack_directory_is_not_reused_after_root_install(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            state = root / 'state'
            state.mkdir()
            ns = runpy.run_path(str(ROOT / 'dist/standalone/host-nvidia.py'))
            publish = ns['publish_linux_driver']
            publish.__globals__['STATE'] = state
            data = {'fingerprint': 'same-payload-and-host', 'linux64': {
                'libEGL_nvidia.so.0': '/usr/lib/libEGL_nvidia.so.777.11.22'}}
            unpacked = root / 'Downloads'
            installed = root / 'opt/releases/hash'
            publish(unpacked, data)
            first = os.readlink(state / 'linux64')
            publish(installed, data)
            self.assertNotEqual(os.readlink(state / 'linux64'), first)
            self.assertEqual(os.readlink(state / 'linux64/libEGL_nvidia.so.0'),
                             str(installed / 'runtime/host/usr/lib/libEGL_nvidia.so.777.11.22'))
            self.assertFalse((state / 'linux64/libc.so.6').exists())

class RuntimePatchTests(unittest.TestCase):
    def test_patch_is_idempotent_and_precedes_cli_entry(self):
        ns = runpy.run_path(str(ROOT / 'scripts/build-runtime.py'))
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for rel in ('usr/lib/waydroid/tools/helpers/nvidia_native.py',
                        'usr/lib/waydroid-nvidia/native/waydroid/nvidia_native.py'):
                path = root / rel
                path.parent.mkdir(parents=True)
                path.write_text('import os\ndef read_ld_cache(path=None): return []\n'
                                'if __name__ == "__main__":\n    print(read_ld_cache())\n')
            ns['patch_host_driver'](root)
            path = root / 'usr/lib/waydroid/tools/helpers/nvidia_native.py'
            first = path.read_text()
            ns['patch_host_driver'](root)
            self.assertEqual(path.read_text(), first)
            self.assertLess(first.index('# NexDroid host-driver adapter'), first.index('if __name__'))
            self.assertEqual(first.count('_nexdroid_original_read_ld_cache ='), 1)

if __name__ == '__main__': unittest.main()
