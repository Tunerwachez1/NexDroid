#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run full Vulkan/media gates and stage a new native root. Does not activate it.
Uses upstream test implementations; native GL is explicitly not staged.
"""
import argparse
import hashlib
import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sys
import time

INSTALLED = Path('/usr/lib/waydroid-nvidia/native')


def load(path,name):
    loader = importlib.machinery.SourceFileLoader(name,str(path))
    spec = importlib.util.spec_from_loader(name,loader)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    loader.exec_module(module)
    return module


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def stage(bundle,work,dest):
    if os.geteuid() != 0:
        raise ValueError('Staging requires root for GPU/media gates and /var/lib; use PolicyKit')
    os.umask(0o022)  # overlay/public directories must remain traversable by Android
    if dest.exists() or dest.parent != Path('/var/lib/waydroid') or not dest.name.startswith('nvidia-source-native-'):
        raise ValueError('Use a NEW /var/lib/waydroid/nvidia-source-native-* root')
    native = load(INSTALLED/'waydroid/nvidia_native.py','repair_native')
    native.ensure_gpu_ready(native=True)
    gate_mod = load(INSTALLED/'packaging/native/release_gate.py','repair_release_gate')
    sn = load(INSTALLED/'dev/source-native','repair_source_native')
    if sn.container_state() != 'STOPPED':
        raise ValueError('Android container must be STOPPED')
    record = gate_mod.verify_bundle(bundle)
    local = record['local_rm_update']
    kernel = native.driver_version()
    if local['version'] != kernel:
        raise ValueError('Locally compiled profile does not match loaded kernel module')
    for target,build in local['builds'].items():
        if sha(build['path']) != build['sha256']:
            raise ValueError('Local build changed: '+target)
    # A nested user namespace running the 32-bit bionic linker cannot traverse
    # a private /home/tim directory. Gate immutable root-owned public copies.
    public = Path('/var/lib/waydroid-nvidia/local-bundles')/(kernel+'-'+time.strftime('%Y%m%d-%H%M%S'))
    public.parent.mkdir(parents=True,exist_ok=True)
    shutil.copytree(bundle,public)
    for p in public.rglob('*'):
        p.chmod(0o755 if p.is_dir() or '/gate/bin/' in str(p) else 0o644)
    public.chmod(0o755)
    bundle = public
    gate_mod.verify_bundle(bundle)
    # The stock image snapshot is read-only test input and is retained for the
    # source-root media validator, even after the live vendor image is replaced.
    original = json.loads(Path('/var/lib/waydroid-nvidia/vendor-image/install.json').read_text())['official_backup']
    source = Path(original['path'])
    if sha(source) != original['sha256']:
        raise ValueError('Original official vendor snapshot hash mismatch')
    image_dir = Path('/var/lib/waydroid/diagnostics')/('native-compat-input-'+time.strftime('%Y%m%d-%H%M%S'))
    image_dir.mkdir(mode=0o700, parents=True)
    vendor = image_dir/'vendor.img'
    shutil.copy2(source,vendor)
    log = Path('/var/lib/waydroid-nvidia')/('gate-local-'+time.strftime('%Y%m%d-%H%M%S'))
    log.mkdir()
    gate = gate_mod.Gate(bundle,log,Path('/var/lib/waydroid/images/system.img'))
    for target in gate_mod.ABIS:
        gate.prepare(target)
    for target in gate_mod.ABIS:
        gate_mod.vulkan_cases(gate,target,'nvidia-prolapse')
    # Media smoke builds its unchanged C++ test against the new GNU RM backend.
    # This uses a separate source copy, never changes installed release tools.
    gate_mod.REPO = work/'gate-tools'
    media,odm = gate_mod.media_case(gate,sn,native,vendor)
    gate_mod.verify_bundle(bundle)
    if native.driver_version() != kernel:
        raise ValueError('Kernel module changed during gate')
    def is_gl(rel):
        return rel.endswith(tuple(native.NVRM_EGL_LIBS)) or rel.endswith(native.NVRM_GALLIUM)
    sources = {r[len('payload/'):]:p for r,p in gate_mod.bundle_files(bundle).items()
               if r.startswith('payload/') and not is_gl(r[len('payload/'):])}
    sources.update(odm)
    angle = {f'vendor/{sub}/egl/{name}':Path('/var/lib/waydroid/nv/guest')/f'vendor/{sub}/egl/{name}'
             for sub in ('lib64','lib') for name in native.EGL_ANGLE}
    sources.update(angle)
    hashes = {r:sha(p) for r,p in sources.items()}
    exercised = [r for r in gate.exercised(gate_mod.ABIS) if r in sources]
    vk = dict(runner='local exact-public-ABI gate (upstream Vulkan/media test implementations)',
              runner_sha256=sha(Path(__file__)),exit=0,kernel_module=kernel,
              finished=gate_mod.now(),log_dir=str(log),cases=gate.cases,
              exercised=exercised,not_exercised=sorted(set(sources)-set(exercised)))
    manifest = dict(format=native.SOURCE_RM_FORMAT,driver='mesa-nvk-rust-rm',
                    mesa_commit=record['mesa_commit'],graphics_abis=['x86_64','x86'],
                    artifacts=hashes,rust_profile='release',
                    rust_builds=record['rust_builds'],local_rm_update=local,
                    sources={r:str(p) for r,p in sources.items()},kernel_module=kernel,gate=vk,media=media,
                    native_gl=dict(enabled=False,reason=local['native_gl']),
                    release=dict(bundle=str(bundle),commit=record['commit'],tag=record['release'],
                                 build_record_sha256=sha(bundle/'BUILD-RECORD.json')))
    try:
        for relative,path in sources.items():
            target = dest/relative
            target.parent.mkdir(parents=True,exist_ok=True)
            shutil.copy2(path,target)
            target.chmod(0o755 if relative.startswith('odm/bin/') else 0o644)
            if sha(target) != hashes[relative]:
                raise ValueError('Staged file differs from gated input: '+relative)
        (dest/native.SOURCE_RM_MANIFEST).write_text(json.dumps(manifest,indent=2)+'\n')
        native._source_rm_nodes(str(dest))
    except BaseException:
        if dest.exists():
            shutil.rmtree(dest)
        raise
    (log/'local-stage-result.json').write_text(json.dumps(dict(root=str(dest),gate=vk,media=media),indent=2)+'\n')
    print('STAGED_AND_CHECKED',dest,flush=True)
    print('GATE_LOG',log,flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle',type=Path,required=True)
    parser.add_argument('--work',type=Path,required=True)
    parser.add_argument('--dest',type=Path,required=True)
    args = parser.parse_args()
    stage(args.bundle.resolve(),args.work.resolve(),args.dest)
