#!/usr/bin/env python3
"""Offline integration checks for a generated NexDroid image bundle (no mounts)."""
# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import json
import pathlib
import subprocess
import sys

bundle = pathlib.Path(sys.argv[1])
repo = pathlib.Path(__file__).resolve().parents[1]
expected_names = {'system.img', 'vendor.img', 'manifest.json'}
checksums = {}
for line in (bundle / 'SHA256SUMS').read_text().splitlines():
    digest, name = line.split(maxsplit=1)
    name = name.lstrip('*')
    if name not in expected_names or name in checksums:
        raise SystemExit('Unexpected/duplicate checksum entry')
    checksums[name] = digest
if checksums.keys() != expected_names:
    raise SystemExit('Incomplete checksums')
for name, digest in checksums.items():
    with (bundle / name).open('rb') as f:
        if hashlib.file_digest(f, 'sha256').hexdigest() != digest:
            raise SystemExit(f'Checksum mismatch: {name}')
manifest = json.loads((bundle / 'manifest.json').read_text())
assert manifest['format'] == 1 and manifest['android'] == 13
assert manifest['userdata_included'] is False
assert manifest['arm'] == ['armeabi-v7a', 'arm64-v8a']
for name in ('system.img', 'vendor.img'):
    subprocess.run(['e2fsck', '-fn', str(bundle / name)], check=True)

def debug(command):
    return subprocess.run(['debugfs', '-R', command, str(bundle / 'system.img')],
                          capture_output=True, check=True).stdout

props = debug('cat /system/build.prop').decode()
for line in ['ro.dalvik.vm.native.bridge=libhoudini.so', 'ro.debuggable=0',
             'ro.secure=1', 'ro.adb.secure=1',
             'ro.product.cpu.abilist=x86_64,x86,arm64-v8a,armeabi-v7a,armeabi']:
    assert line in props.splitlines(), line
for path in ['bin/houdini', 'bin/houdini64', 'lib/libhoudini.so',
             'lib64/libhoudini.so', 'lib/arm/libc.so', 'lib64/arm64/libc.so',
             'etc/init/houdini.rc']:
    assert b'Inode:' in debug(f'stat /system/{path}'), path
    assert b'security.selinux' in debug(f'ea_list /system/{path}'), path
apk = debug('cat /system/app/NexDroidDesktop/NexDroidDesktop.apk')
assert hashlib.sha256(apk).digest() == hashlib.sha256(
    (repo / 'android/desktop/prebuilt/waydroid-desktop.apk').read_bytes()).digest()
for path in ['/system/bin/su', '/system/xbin/su', '/system/etc/init/nexdroid-root.rc',
             '/system/etc/nexdroid-root']:
    assert b'Inode:' not in debug(f'stat {path}'), path
data = debug('ls -p /data').decode().splitlines()
assert all(not line or line.split('/')[5] in ('.', '..') for line in data), data
print('PASS: hashes, ext filesystems, ARM32/64, SELinux labels, exact Desktop APK, empty /data, no NexDroid root hooks')
