#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Prepare optional, official Magisk 31.0 payload; does not enable root."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import tempfile
import zipfile

SHA256 = '2c8a488b9a5293e578e95ae4f07e3c57aba4feec4a52ca4dd852a2692d6dd4e8'
DEST = Path('/var/lib/nexdroid/root')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apk', required=True, type=Path, help='Official Magisk-v31.0.apk')
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error('Run with sudo; preparing the payload does not enable root')
    data = args.apk.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA256:
        parser.error('APK digest is not the official Magisk 31.0 digest')
    arch = {'x86_64': ('x86_64', 'x86'), 'aarch64': ('arm64-v8a', 'armeabi-v7a')}.get(platform.machine())
    if arch is None:
        parser.error('Supported host architectures: x86_64 and aarch64')
    if (DEST / 'runtime').exists() or DEST.is_symlink():
        parser.error('Runtime already exists; preserve the existing installation and its manager identity')
    if any(Path('/var/lib/waydroid').glob('magisk-disabled-*/manifest.json')):
        parser.error('An existing disabled Magisk installation is present; use the NexDroid root toggle to preserve its manager identity')
    DEST.mkdir(parents=True, exist_ok=True, mode=0o700)
    DEST.chmod(0o700)
    with zipfile.ZipFile(args.apk) as apk, tempfile.TemporaryDirectory(dir=DEST) as temp:
        target = Path(temp) / 'runtime'
        target.mkdir(mode=0o700)
        entries = {'magisk': f'lib/{arch[0]}/libmagisk.so', 'busybox': f'lib/{arch[0]}/libbusybox.so', 'stub.apk': 'assets/stub.apk'}
        for name in ('magiskboot', 'magiskinit', 'magiskpolicy', 'bootctl'):
            entries[name] = f'lib/{arch[0]}/lib{name}.so'
        entries['init-ld'] = f'lib/{arch[0]}/libinit-ld.so'
        entries['libinit-ld.so'] = entries['init-ld']
        entries['magisk32'] = f'lib/{arch[1]}/libmagisk.so'
        entries['libinit-ld32.so'] = f'lib/{arch[1]}/libinit-ld.so'
        for name in ('addon.d.sh', 'app_functions.sh', 'boot_patch.sh', 'module_installer.sh', 'uninstaller.sh', 'util_functions.sh'):
            entries[name] = f'assets/{name}'
        for name, source in entries.items():
            if source not in apk.namelist():
                if name in ('magisk', 'busybox', 'stub.apk'):
                    parser.error(f'Required payload is missing: {source}')
                continue
            path = target / name
            path.write_bytes(apk.read(source))
            path.chmod(0o600 if name.endswith('.apk') else 0o700)
        (target / 'magisk.apk').write_bytes(data)
        (target / 'magisk.apk').chmod(0o600)
        target.rename(DEST / 'runtime')
    (DEST / 'runtime.json').write_text(json.dumps({'manager_package': 'com.topjohnwu.magisk'}) + '\n')
    (DEST / 'runtime.json').chmod(0o600)
    print('Runtime prepared. Enable Root (Magisk) in NexDroid Advanced settings and restart Android.')


if __name__ == '__main__':
    main()
