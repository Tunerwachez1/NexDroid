#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fresh installs only. Re-gate the bundled exact-driver GPU payload on this GPU."""
import configparser
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

BASE = pathlib.Path('/opt/nexdroid/current')
WORK = pathlib.Path('/var/lib/waydroid')

def check_version(base=BASE):
    proc = pathlib.Path('/proc/driver/nvidia/version')
    if not proc.exists():
        return None
    version = re.search(r'NVRM version:.*?\s(\d+\.\d+(?:\.\d+)?)\b', proc.read_text())
    assert version, 'Cannot identify the loaded NVIDIA driver'
    version = version.group(1)
    record = json.loads((base / 'gpu/bundle/BUILD-RECORD.json').read_text())
    expected = record['local_rm_update']['version']
    if version != expected:
        raise SystemExit(f'NVIDIA {version}: this beta bundle contains profile {expected}. '
                         'A compatible GPU payload is required; no version check is bypassed.')
    return version

def provision():
    version = check_version()
    if version is None:
        return
    assert os.geteuid() == 0
    stamp = time.strftime('%Y%m%d-%H%M%S')
    original = BASE / 'images/vendor.img'
    install = pathlib.Path('/var/lib/waydroid-nvidia/vendor-image/install.json')
    # Gate the exact shipped base: e2fsck changes filesystem metadata/checksum
    # even when its contents match the upstream original vendor snapshot.
    # Never replace existing installation records on a migrated host.
    if not install.exists():
        install.parent.mkdir(parents=True, exist_ok=True)
        with original.open('rb') as f:
            digest = hashlib.file_digest(f, 'sha256').hexdigest()
        install.write_text(json.dumps({'format': 2, 'official_backup': {
            'path': str(original), 'sha256': digest}}, indent=2))
    guest = WORK / 'nv/guest'
    assert not guest.exists(), 'Fresh provisioning refuses to overwrite existing GPU guest files'
    guest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(BASE / 'gpu/guest', guest, symlinks=True)
    dest = WORK / f'nvidia-source-native-nexdroid-{version}-{stamp}'
    subprocess.run([sys.executable, str(BASE / 'stage-native.py'), '--bundle', str(BASE / 'gpu/bundle'),
                    '--work', str(BASE / 'gpu'), '--dest', str(dest)], check=True)
    # The active validator requires the live vendor path, not the gate's
    # snapshot path. Repoint provenance only after proving byte identity.
    manifestfile = dest / 'source-rm.json'
    manifest = json.loads(manifestfile.read_text())
    snapshot = pathlib.Path(manifest['media']['vendor_img'])
    live = BASE / 'images/vendor.img'
    def sha(path):
        with path.open('rb') as f:
            return hashlib.file_digest(f, 'sha256').hexdigest()
    assert sha(snapshot) == sha(live), 'Live vendor differs from the gated vendor snapshot'
    manifest['media']['staged_vendor_snapshot'] = str(snapshot)
    manifest['media']['vendor_img'] = str(live)
    manifestfile.write_text(json.dumps(manifest, indent=2) + '\n')
    # This root has passed the full Vulkan and media gates on this machine.
    link = WORK / 'nvidia-prolapse'
    assert not link.exists() and not link.is_symlink(), 'GPU active link already exists'
    link.symlink_to(dest)
    cfgfile = WORK / 'waydroid.cfg'
    cfg = configparser.ConfigParser()
    cfg.optionxform = str
    cfg.read(cfgfile)
    sys.path.insert(0, '/usr/lib/waydroid')
    import tools
    from tools.helpers import nvidia_native
    cfg['waydroid'].update(nvidia_driver_mode='nvidia-prolapse', nvidia_native_gl='off',
        nvidia_prolapse_install='dev-root', nvidia_guest_layout='2', suspend_action='none',
        drm_device=nvidia_native.nvidia_render_node())
    with cfgfile.open('w') as f:
        cfg.write(f)
    # Regenerate just the configuration; do not call init --force/upgrade or
    # remove any overlays/data. The private Waydroid generates its own mounts.
    sys.path.insert(0, '/usr/lib/waydroid')
    import tools
    sys.argv = ['waydroid', 'status']
    args = tools.helpers.arguments()
    tools.prep_args(args)
    from tools.helpers import logging as tools_logging
    tools_logging.init(args)
    cfg = tools.config.load(args)
    args.arch = cfg['waydroid']['arch']
    args.vendor_type = cfg['waydroid']['vendor_type']
    args.images_path = cfg['waydroid']['images_path']
    tools.helpers.drivers.setupBinderNodes(args)
    tools.helpers.lxc.set_lxc_config(args)
    tools.helpers.lxc.make_base_props(args)
    subprocess.run([sys.executable, '/usr/lib/waydroid-nvidia/native/dev/driver-mode', 'verify-active'], check=True)

if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--check-version':
        check_version(pathlib.Path(sys.argv[2]))
    else:
        provision()
