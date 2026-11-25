#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Hardware-gated host-driver preparation. No exact-release RM profile needed."""
import configparser
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

BASE = Path(__file__).resolve().parent
WORK = Path('/var/lib/waydroid')
STATE = Path('/var/lib/nexdroid/gpu')

def load_shim():
    path = BASE / 'runtime/usr/lib/waydroid-nvidia/native/packaging/native/glibc_shim.py'
    spec = importlib.util.spec_from_file_location('nexdroid_glibc_shim', path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod

def regenerate():
    sys.path.insert(0, '/usr/lib/waydroid')
    import tools
    sys.argv = ['waydroid', 'status']
    args = tools.helpers.arguments()
    tools.prep_args(args)
    from tools.helpers import logging as tools_logging
    tools_logging.init(args)
    cfg = tools.config.load(args)
    for name in ('arch', 'vendor_type', 'images_path', 'system_ota', 'vendor_ota'):
        setattr(args, name, cfg['waydroid'].get(name, 'None'))
    tools.helpers.drivers.setupBinderNodes(args)
    tools.helpers.lxc.set_lxc_config(args)
    tools.helpers.lxc.make_base_props(args)

def prepare():
    if os.geteuid() != 0: raise RuntimeError('GPU preparation requires root')
    STATE.mkdir(parents=True, exist_ok=True)
    with (STATE / 'prepare.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        _prepare()

def _prepare():
    shim = load_shim()
    bundle = BASE / 'gpu/bundle'
    version, closures, files = shim.check(bundle, False)
    host = json.loads((STATE / 'host-driver.json').read_text())
    record = STATE / 'ready.json'
    if record.exists():
        ready = json.loads(record.read_text())
        if (ready.get('fingerprint') == host['fingerprint'] and
                Path(shim.native.ROOT_LINKS['glibc-shim']).resolve() == Path(ready['root']) and
                'nvidia_driver_mode = glibc-shim' in (WORK / 'waydroid.cfg').read_text()):
            subprocess.run([sys.executable, '/usr/lib/waydroid-nvidia/native/dev/driver-mode',
                            'verify-active'], check=True)
            return
    root = WORK / ('nexdroid-glibc-shim-' + host['fingerprint'][:16] + '-' + str(time.time_ns()))
    for rel, src in files.items():
        if rel.startswith('root/'): shim.put(src, root / rel[5:])
    for bits, (_, sub) in shim.ARCHES.items():
        lib = root / 'vendor_extra' / sub
        (lib / 'host-glibc-version').write_text(host['glibc'].split()[-1] + '\n')
        for name, _ in closures[bits]: (lib / name).touch(mode=0o644)
        (lib / shim.native.ICD_SONAME).symlink_to(shim.native.ICD_FILE.format(v=version))
        for name in shim.native.EGL_ANGLE: (root / 'vendor' / sub / 'egl' / name).touch()
    # Gate first. Failure leaves live config, links and held application data intact.
    shim.gate(bundle, root, closures, images=WORK / 'images', log_root=STATE)
    # Refuse a driver-library update racing the gate, including unchanged version.
    import hashlib
    for source, expected in host['files'].items():
        with (BASE / 'runtime/host' / source.lstrip('/')).open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != expected:
                raise RuntimeError('NVIDIA libraries changed during GPU tests; retry preparation')
    guest = WORK / 'nv/guest'
    if not guest.exists():
        guest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(BASE / 'gpu/guest', guest, symlinks=True)
    shim.native.lxc_nodes(str(root), native_gl=False)
    cfgfile = WORK / 'waydroid.cfg'
    cfg = configparser.ConfigParser()
    cfg.optionxform = str
    cfg.read(cfgfile)
    oldcfg = cfgfile.read_bytes()
    cfg['waydroid'].update(nvidia_driver_mode='glibc-shim', nvidia_native_gl='off',
        nvidia_guest_layout='2', suspend_action='none', drm_device=shim.native.nvidia_render_node())
    for key in ('nvidia_prolapse_install', 'nvidia_native_root', 'nvidia_rust_driver_root', 'nvidia_glibc_shim_root'):
        cfg['waydroid'].pop(key, None)
    link = Path(shim.native.ROOT_LINKS['glibc-shim'])
    previous = os.readlink(link) if link.is_symlink() else None
    if link.exists() and not link.is_symlink(): raise RuntimeError('GPU link is occupied by a directory')
    try:
        shim.native.repoint_link(str(link), str(root))
        temp = cfgfile.with_suffix('.nexdroid-new')
        with temp.open('w') as stream: cfg.write(stream)
        os.replace(temp, cfgfile)
        regenerate()
        subprocess.run([sys.executable, '/usr/lib/waydroid-nvidia/native/dev/driver-mode',
                        'verify-active'], check=True)
    except Exception:
        cfgfile.write_bytes(oldcfg)
        if previous is not None: shim.native.repoint_link(str(link), previous)
        elif link.is_symlink(): link.unlink()
        regenerate()
        raise
    temp = record.with_suffix('.tmp')
    temp.write_text(json.dumps({'format': 1, 'mode': 'glibc-shim', 'driver': version,
        'fingerprint': host['fingerprint'], 'root': str(root)}, indent=2) + '\n')
    os.replace(temp, record)
    print('NexDroid: host NVIDIA driver prepared and hardware-verified: ' + version)

if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['check']:
            load_shim().check(BASE / 'gpu/bundle', False)
        elif sys.argv[1:] == ['prepare']: prepare()
        else: raise RuntimeError('Expected check or prepare')
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit('NexDroid NVIDIA: ' + str(error))
