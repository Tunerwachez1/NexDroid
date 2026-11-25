#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Maintainer packaging. End users only open the generated NexDroid executable."""
import argparse
import hashlib
import importlib.util
import json
import os
import pathlib
import shutil
import subprocess

REPO = pathlib.Path(__file__).resolve().parents[1]

def clone(src, dst):
    subprocess.run(['cp', '-a', '--no-preserve=ownership', '--reflink=auto', str(src), str(dst)], check=True)

def build(args):
    assert os.geteuid() == 0, 'Maintainer packaging requires sudo to read the original vendor snapshot'
    out = args.out.resolve()
    assert not out.exists(), 'Use a new output directory'
    out.mkdir(parents=True)
    clone(args.runtime.resolve(), out / 'runtime')
    spec = importlib.util.spec_from_file_location('nexdroid_runtime_builder', REPO / 'scripts/build-runtime.py')
    runtime_builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runtime_builder)
    runtime_builder.patch_host_driver(out / 'runtime')
    runtime_builder.patch_clipboard(out / 'runtime')
    runtime_builder.patch_post_android_boot(out / 'runtime')
    clone(REPO / 'dist/android', out / 'images')
    for src in (REPO / 'dist/standalone').iterdir():
        if not src.is_file(): continue
        target = out / ('launch' if src.name == 'NexDroid' else src.name)
        shutil.copyfile(src, target)
        target.chmod(0o755 if src.suffix != '.py' else 0o644)
    shutil.copyfile(REPO / 'target/release/nexdroid-launcher', out / 'NexDroid')
    (out / 'NexDroid').chmod(0o755)
    for path in ['usr/lib/modules', 'usr/lib/firmware', 'usr/share/fonts', 'usr/share/icons']:
        (out / 'runtime' / path).mkdir(parents=True, exist_ok=True)
    (out / 'runtime/usr/bin/nvidia-modprobe').touch(exist_ok=True)
    (out / 'bin').mkdir()
    for name in ['liw-ui', 'liwd', 'liwd-helper', 'liw']:
        shutil.copyfile(REPO / 'target/release' / name, out / 'bin' / name)
        (out / 'bin' / name).chmod(0o755)
    (out / 'host-bin').mkdir()
    (out / 'host-bin/waydroid').write_text('#!/bin/bash\nexec /opt/nexdroid/current/runtime-exec /usr/bin/waydroid "$@"\n')
    (out / 'host-bin/waydroid').chmod(0o755)
    integration = out / 'integration'
    integration.mkdir()
    base = '/opt/nexdroid/current'
    def unit(name, body):
        (integration / name).write_text(body)
    unit('nexdroid-container.service', f'''[Unit]
Description=NexDroid bundled Android container
After=dbus.service
[Service]
Type=dbus
BusName=id.waydro.Container
UMask=0022
ExecStartPre={base}/host-python {base}/prepare-binder.py
ExecStartPre={base}/host-python {base}/host-nvidia.py prepare
ExecStartPre={base}/runtime-exec /usr/bin/python3 {base}/startup-check.py
ExecStart={base}/runtime-exec /usr/bin/waydroid container start
''')
    helper_unit = (REPO / 'dist/systemd/liwd-helper.service').read_text()
    unit('liwd-helper.service', helper_unit.replace('/usr/local/bin/liwd-helper',
         f'{base}/runtime-exec {base}/bin/liwd-helper').replace('RestrictSUIDSGID=yes',
         '# RestrictSUIDSGID blocks Bubblewrap open_tree with ENOSYS.\nRestrictSUIDSGID=no'))
    unit('liwd.service', f'''[Unit]
Description=NexDroid session manager
After=graphical-session.target
PartOf=graphical-session.target
[Service]
Type=exec
ExecStart={base}/runtime-exec {base}/bin/liwd
Restart=on-failure
RestartSec=5
KillMode=process
''')
    unit('nexdroid-keyboard.service', f'''[Unit]
Description=NexDroid keyboard layout bridge
After=graphical-session.target
PartOf=graphical-session.target
[Service]
Type=simple
ExecStart={base}/runtime-exec /usr/bin/python3 {base}/integration/layout_sync.py
Restart=on-failure
RestartSec=2
[Install]
WantedBy=graphical-session.target
''')
    for name, bus, service, executable in [
        ('container', 'id.waydro.Container', 'waydroid-container.service', '/usr/bin/waydroid -w container start'),
        ('helper', 'id.liwinux.Helper1', 'liwd-helper.service', f'{base}/bin/liwd-helper')]:
        unit(name + '.dbus-service', f'[D-BUS Service]\nName={bus}\nExec={base}/runtime-exec {executable}\nUser=root\nSystemdService={service}\n')
    shutil.copyfile(REPO / 'dist/dbus/id.liwinux.Helper1.conf', integration / 'helper.dbus-policy')
    shutil.copyfile('/usr/share/dbus-1/system.d/id.waydro.Container.conf', integration / 'container.dbus-policy')
    shutil.copyfile(REPO / 'dist/polkit/id.liwinux.policy', integration / 'id.liwinux.policy')
    shutil.copyfile(REPO / 'dist/config/embedded.toml', integration / 'embedded.toml')
    shutil.copyfile(REPO / 'android/desktop/host/layout_sync.py', integration / 'layout_sync.py')
    shutil.copyfile(REPO / 'assets/waytrivo.png', integration / 'waytrivo.png')
    unit('runtime-override.conf', f'''[Service]
ExecStart=
ExecStartPre=
ExecStartPre={base}/host-python {base}/prepare-binder.py
ExecStartPre={base}/host-python {base}/host-nvidia.py prepare
ExecStartPre={base}/runtime-exec /usr/bin/python3 {base}/startup-check.py
ExecStart={base}/runtime-exec /usr/bin/waydroid container start
''')
    gpu = out / 'gpu'
    gpu.mkdir()
    clone(args.gpu_bundle.resolve(), gpu / 'bundle')
    clone('/var/lib/waydroid/nv/guest', gpu / 'guest')
    install = json.loads(pathlib.Path('/var/lib/waydroid-nvidia/vendor-image/install.json').read_text())
    original = pathlib.Path(install['official_backup']['path'])
    with original.open('rb') as f:
        assert hashlib.file_digest(f, 'sha256').hexdigest() == install['official_backup']['sha256']
    clone(original, gpu / 'official-vendor.img')
    (gpu / 'official-vendor.img').chmod(0o644)
    media = args.native_build.resolve() / 'gate-tools/nvidia-prolapse/media'
    host = args.native_build.resolve() / 'gate-tools/build/nv-rm-rust/release/libnv_bionic_native.so'
    (gpu / 'gate-tools/nvidia-prolapse').mkdir(parents=True)
    clone(media, gpu / 'gate-tools/nvidia-prolapse/media')
    assert (gpu / 'gate-tools/nvidia-prolapse/media/out/host/real_codec_test').is_file()
    hostdest = gpu / 'gate-tools/build/nv-rm-rust/release/libnv_bionic_native.so'
    hostdest.parent.mkdir(parents=True)
    clone(host, hostdest)
    recordfile = gpu / 'bundle/BUILD-RECORD.json'
    record = json.loads(recordfile.read_text())
    paths = {'android': 'gpu/bundle/payload/vendor_extra/lib64/libnv_bionic_native.so',
             'android32': 'gpu/bundle/payload/vendor_extra/lib/libnv_bionic_native.so',
             'host': 'gpu/gate-tools/build/nv-rm-rust/release/libnv_bionic_native.so'}
    for target, relative in paths.items():
        info = record['local_rm_update']['builds'][target]
        with (out / relative).open('rb') as f:
            assert hashlib.file_digest(f, 'sha256').hexdigest() == info['sha256'], target
        info['path'] = base + '/' + relative
    recordfile.write_text(json.dumps(record, indent=2) + '\n')
    # Include our sources alongside binaries; signing keys and generated data
    # are deliberately outside git ls-files and the explicit new source list.
    sources = out / 'Sources'
    tracked = subprocess.check_output(['git', '-c', f'safe.directory={REPO}', 'ls-files', '-z'], cwd=REPO).decode().split('\0')
    extra = [str(p.relative_to(REPO)) for p in (REPO / 'dist/standalone').iterdir() if p.is_file()]
    extra += ['scripts/build-runtime.py', 'scripts/build-standalone.py', 'scripts/check-standalone-fresh.sh',
              'scripts/test-standalone-payload.py', 'scripts/test-host-nvidia.py', 'scripts/check-standalone-shim.sh', 'scripts/check-host-graphics.py', 'docs/standalone.md', 'docs/nvidia-host-driver.md']
    extra += [str(p.relative_to(REPO)) for p in (REPO / 'crates/nexdroid-launcher').rglob('*') if p.is_file()]
    for rel in sorted(set(tracked + extra) - {''}):
        src = REPO / rel
        assert '.signing' not in src.parts
        dst = sources / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)
    entries = {}
    for p in sorted(out.rglob('*')):
        rel = str(p.relative_to(out))
        if p.is_symlink():
            entries[rel] = {'symlink': os.readlink(p)}
        elif p.is_file():
            with p.open('rb') as f:
                entries[rel] = {'sha256': hashlib.file_digest(f, 'sha256').hexdigest(),
                                'exec': p.stat().st_mode & 0o111}
    data = (json.dumps({'format': 1, 'files': entries}, sort_keys=True, indent=2) + '\n').encode()
    (out / 'payload.json').write_bytes(data)
    (out / 'payload-id').write_text(hashlib.sha256(data).hexdigest() + '\n')
    print(f'Standalone payload assembled: {out} ({len(entries)} verified entries)')

if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for opt in ['runtime', 'gpu-bundle', 'native-build', 'out']:
        p.add_argument('--' + opt, required=True, type=pathlib.Path)
    build(p.parse_args())
