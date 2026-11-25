#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Maintainer-only export. Never exports Android /data, overlay_rw or host config.
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
[[ $EUID == 0 ]] || { echo 'Run this image builder with sudo.' >&2; exit 1; }
[[ $# == 1 ]] || { echo 'Usage: sudo bash scripts/build-android-bundle.sh NEW_OUTPUT_DIRECTORY' >&2; exit 2; }
out=$(realpath -m -- "$1")
[[ ! -e $out ]] || { echo 'Output must not exist.' >&2; exit 1; }
for tool in mount umount unshare e2fsck resize2fs python3 sha256sum; do command -v "$tool" >/dev/null; done
# Private mount namespace: never mount or modify the live Android images.
if [[ ${NEXDROID_PRIVATE_MOUNTS:-} != 1 ]]; then
    exec unshare --mount --propagation private env NEXDROID_PRIVATE_MOUNTS=1 bash "$0" "$out"
fi
apk="$repo/android/desktop/prebuilt/waydroid-desktop.apk"
(cd "$(dirname "$apk")"; sha256sum -c waydroid-desktop.apk.sha256)
mkdir -p -- "$out"
scratch=$(mktemp -d)
mounted=false
cleanup() { if "$mounted"; then umount "$scratch/system"; fi; rmdir "$scratch/system" "$scratch" 2>/dev/null || true; }
trap cleanup EXIT
for image in system vendor; do cp --reflink=auto --sparse=always "/var/lib/waydroid/images/$image.img" "$out/$image.img"; done
checkfs() { local rc=0; e2fsck -pf "$1" || rc=$?; ((rc <= 1)); }
checkfs "$out/system.img"
truncate -s +512M "$out/system.img"
resize2fs "$out/system.img"
mkdir "$scratch/system"
mount -o loop,rw,nosuid,nodev,noexec "$out/system.img" "$scratch/system"
mounted=true
system="$scratch/system/system"
[[ -f $system/build.prop && -d $system/app ]] || { echo 'Unexpected system image layout.' >&2; exit 1; }
# Deliberate allowlist. In particular, do NOT copy etc/nexdroid-root or init/*.rc.
for entry in bin/houdini bin/houdini64 bin/arm bin/arm64 lib/libhoudini.so lib/arm lib64/libhoudini.so lib64/arm64 etc/binfmt_misc etc/init/houdini.rc; do
    src="/var/lib/waydroid/overlay/system/$entry"
    [[ -e $src ]] || { echo "Missing required ARM component: $entry" >&2; exit 1; }
    mkdir -p "$(dirname "$system/$entry")"
    cp -a -- "$src" "$(dirname "$system/$entry")/"
done
install -d -m755 "$system/app/NexDroidDesktop"
install -m644 "$apk" "$system/app/NexDroidDesktop/NexDroidDesktop.apk"
python3 - "$system/build.prop" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
props = {
    'ro.product.cpu.abilist': 'x86_64,x86,arm64-v8a,armeabi-v7a,armeabi',
    'ro.product.cpu.abilist32': 'x86,armeabi-v7a,armeabi',
    'ro.product.cpu.abilist64': 'x86_64,arm64-v8a',
    'ro.dalvik.vm.native.bridge': 'libhoudini.so',
    'ro.enable.native.bridge.exec': '1', 'ro.enable.native.bridge.exec64': '1',
    'ro.vendor.enable.native.bridge.exec': '1', 'ro.vendor.enable.native.bridge.exec64': '1',
    'ro.dalvik.vm.isa.arm': 'x86', 'ro.dalvik.vm.isa.arm64': 'x86_64',
    'ro.debuggable': '0', 'ro.secure': '1', 'ro.adb.secure': '1',
}
lines = [line for line in p.read_text().splitlines() if line.partition('=')[0] not in props]
p.write_text('\n'.join(lines) + '\n# NexDroid bundled Android\n' + ''.join(f'{k}={v}\n' for k,v in props.items()))
PY
# Host overlay files have no Android SELinux xattrs; label the baked copies.
python3 - "$system" <<'PY'
import os, pathlib, sys
root = pathlib.Path(sys.argv[1])
entries = ['bin/houdini', 'bin/houdini64', 'bin/arm', 'bin/arm64',
           'lib/libhoudini.so', 'lib/arm', 'lib64/libhoudini.so', 'lib64/arm64',
           'etc/binfmt_misc', 'etc/init/houdini.rc', 'app/NexDroidDesktop']
for entry in entries:
    path = root / entry
    label = b'u:object_r:system_lib_file:s0\0' if entry.startswith(('lib/', 'lib64/')) else b'u:object_r:system_file:s0\0'
    for p in [path, *(path.rglob('*') if path.is_dir() else [])]:
        if not p.is_symlink():
            os.setxattr(p, 'security.selinux', label)
PY
sync -f "$out/system.img"
umount "$scratch/system"
mounted=false
checkfs "$out/system.img"
checkfs "$out/vendor.img"
python3 - "$out/manifest.json" <<'PY'
import json, sys
from datetime import datetime, timezone
with open(sys.argv[1], 'w') as f:
    json.dump(dict(format=1, name='NexDroid Android', android=13, arch='x86_64',
        variant='GAPPS', desktop_version='0.8', arm=['armeabi-v7a','arm64-v8a'],
        userdata_included=False, nvidia_host_backend_included=False,
        created_at=datetime.now(timezone.utc).isoformat()), f, indent=2)
PY
(cd "$out"; sha256sum system.img vendor.img manifest.json > SHA256SUMS)
chmod 644 "$out/"*
echo "Prepared Android bundle: $out (not boot-tested on a fresh machine)."
