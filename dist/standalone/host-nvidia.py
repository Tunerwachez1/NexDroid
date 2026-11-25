#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Discover NVIDIA outside the private /usr, then gate it inside the runtime."""
import argparse
import configparser
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile

STATE = Path('/var/lib/nexdroid/gpu')

def load(path):
    spec = importlib.util.spec_from_file_location('host_nvidia_native', path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod

def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def snapshot(base, abis=(64, 32)):
    # host-python runs this in the original host mount namespace.
    os.environ.pop('NEXDROID_HOST_DRIVER_ROOT', None)
    os.environ.pop('NEXDROID_HOST_DRIVER_MANIFEST', None)
    native = load(base / 'runtime/usr/lib/waydroid/tools/helpers/nvidia_native.py')
    version = native.driver_version()
    cache = native.read_ld_cache()
    files = {}
    for bits in abis:
        for name, source in native.host_closure(version, bits, cache):
            source = Path(source).resolve(strict=True)
            if not str(source).startswith(('/usr/', '/lib/', '/lib64/')):
                raise RuntimeError('Unsupported host NVIDIA library location: ' + str(source))
            files[str(source)] = digest(source)
    entries = [(flags, name, str(Path(source).resolve(strict=True)))
               for flags, name, source in cache
               if name.startswith(('libEGL_nvidia', 'libGLX_nvidia', 'libnvidia-')) and Path(source).exists()]
    linux = {name: source for flags, name, source in entries if flags == native.LD_CACHE_FLAGS[64]}
    for source in linux.values():
        if source not in files: files[source] = digest(source)
    glx = linux.get('libGLX_nvidia.so.0')
    if glx and Path(glx).name != 'libGLX_nvidia.so.' + version:
        raise RuntimeError('Linux GLX/Vulkan userspace does not match the loaded NVIDIA module')
    vulkan = None
    for icd in sorted(Path('/usr/share/vulkan/icd.d').glob('*nvidia*.json')):
        if any(arch in icd.name for arch in ('i686', 'i386', 'aarch64')): continue
        candidate = json.loads(icd.read_text())
        name = Path(candidate.get('ICD', {}).get('library_path', '')).name
        if name in linux:
            candidate['ICD']['library_path'] = str(STATE / 'linux64' / name)
            vulkan = candidate
            break
    pci = {str(p.parent.name): p.read_text().strip() for p in Path('/sys/bus/pci/devices').glob('*/device')
           if (p.parent / 'vendor').read_text().strip() == '0x10de'}
    glibc = subprocess.run(['/usr/bin/getconf', 'GNU_LIBC_VERSION'], capture_output=True,
                           text=True, check=True).stdout.strip()
    if not glibc.startswith('glibc '): raise RuntimeError('Cannot identify the host glibc')
    data = {'format': 1, 'driver': version, 'cache': sorted(entries), 'files': files,
            'gpu': pci, 'glibc': glibc, 'linux64': linux, 'vulkan': vulkan,
            'bundle': digest(base / 'gpu/bundle/BUILD-RECORD.json'),
            'payload': (base / 'payload-id').read_text().strip()}
    data['fingerprint'] = hashlib.sha256(json.dumps(data, sort_keys=True).encode()).hexdigest()
    return data

def atomic_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
    fd, temp = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(data, stream, sort_keys=True, indent=2)
            stream.write('\n')
        os.chmod(temp, 0o644)
        os.replace(temp, path)
    finally:
        if os.path.exists(temp): os.unlink(temp)

def publish_linux_driver(base, data):
    """NVIDIA-only lookup directory: never put the host libc ahead of private libc."""
    origin = hashlib.sha256(str(base.resolve()).encode()).hexdigest()[:12]
    target = STATE / ('linux64-' + data['fingerprint'] + '-' + origin)
    if not target.exists():
        stage = Path(tempfile.mkdtemp(prefix='.linux64-', dir=STATE))
        try:
            for name, source in data['linux64'].items():
                (stage / name).symlink_to(base / 'runtime/host' / source.lstrip('/'))
            allocator = data['linux64'].get('libnvidia-allocator.so.1')
            if allocator:
                (stage / 'nvidia-drm_gbm.so').symlink_to(base / 'runtime/host' / allocator.lstrip('/'))
            mesa = base / 'runtime/usr/lib/gbm/dri_gbm.so'
            if mesa.exists(): (stage / 'dri_gbm.so').symlink_to(mesa)
            stage.chmod(0o755)
            os.rename(stage, target)
        finally:
            if stage.exists():
                import shutil
                shutil.rmtree(stage)
    link = STATE / '.linux64-new'
    link.unlink(missing_ok=True)
    link.symlink_to(target)
    os.replace(link, STATE / 'linux64')
    if data.get('vulkan'): atomic_json(STATE / 'vulkan-nvidia.json', data['vulkan'])
    else: (STATE / 'vulkan-nvidia.json').unlink(missing_ok=True)

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('action', choices=('inspect', 'preflight', 'prepare', 'migrate'))
    ap.add_argument('--base', type=Path, default=Path(__file__).resolve().parent)
    args = ap.parse_args()
    if not Path('/proc/driver/nvidia/version').exists(): return
    cfg = configparser.ConfigParser()
    cfg.read('/var/lib/waydroid/waydroid.cfg')
    mode = cfg.get('waydroid', 'nvidia_driver_mode', fallback='')
    keep_native = args.action == 'prepare' and mode not in ('', 'glibc-shim')
    if args.action != 'inspect' and os.geteuid() != 0:
        raise RuntimeError('NexDroid NVIDIA preparation requires the system service')
    data = snapshot(args.base, abis=(64,) if keep_native else (64, 32))
    if args.action == 'inspect':
        print(json.dumps(data, indent=2)); return
    if args.action in ('prepare', 'migrate'):
        active = subprocess.run([str(args.base / 'runtime-exec'), '/usr/bin/lxc-info',
            '-P', '/var/lib/waydroid/lxc', '-n', 'waydroid', '-sH'], capture_output=True, text=True)
        if active.stdout.strip() == 'RUNNING':
            raise RuntimeError('Close the Android session before GPU preparation')
    atomic_json(STATE / 'host-driver.json', data)
    publish_linux_driver(args.base, data)
    if keep_native: return
    command = [str(args.base / 'runtime-exec'), '/usr/bin/python3', str(args.base / 'shim-gpu.py'),
               'check' if args.action == 'preflight' else 'prepare']
    subprocess.run(command, check=True)

if __name__ == '__main__':
    try: main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit('NexDroid NVIDIA: ' + str(error))
