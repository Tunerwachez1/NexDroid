#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Real host driver + isolated fresh Android config. Never starts/stops a host unit.
set -euo pipefail
[[ $EUID == 0 && $# == 2 ]] || { echo 'Usage: sudo bash scripts/check-standalone-shim.sh BUNDLE NEW_WORK_DIRECTORY'; exit 2; }
base=$(realpath "$1")
work=$(realpath -m "$2")
check=$(realpath "$(dirname "$0")/check-host-graphics.py")
[[ -f "$base/payload-id" && ! -e $work ]] || exit 1
mkdir -p "$work/android" "$work/state" "$work/gpu"
root="$base/runtime"
exec "$root/usr/lib/ld-linux-x86-64.so.2" --library-path "$root/usr/lib" \
    "$root/usr/bin/bwrap" --bind / / --dev-bind /dev /dev --cap-add ALL --unshare-net \
    --tmpfs /run/lxc --tmpfs /run/waydroid \
    --tmpfs /opt --ro-bind "$base" /opt/nexdroid/releases/test-payload \
    --bind "$work/android" /var/lib/waydroid \
    --bind "$work/state" /var/lib/nexdroid \
    --bind "$work/gpu" /var/lib/waydroid-nvidia \
    -- /bin/bash -ec '
        ln -s releases/test-payload /opt/nexdroid/current
        base=/opt/nexdroid/current
        "$base/host-python" "$base/host-nvidia.py" preflight
        ln -s "$base/images" /var/lib/waydroid/images
        "$base/runtime-exec" /usr/bin/waydroid init -i "$base/images" -s GAPPS
        # Existing installs commonly use this alias rather than a preinstalled-images path.
        "$base/runtime-exec" /usr/bin/python3 -c '"'"'import configparser; p="/var/lib/waydroid/waydroid.cfg"; c=configparser.ConfigParser(); c.read(p); c["waydroid"]["images_path"]="/var/lib/waydroid/images"; f=open(p,"w"); c.write(f); f.close()'"'"'
        "$base/host-python" "$base/host-nvidia.py" prepare
        "$base/runtime-exec" /usr/bin/python3 "$base/startup-check.py"
        "$base/host-python" "$base/host-nvidia.py" prepare
        "$base/runtime-exec" /usr/bin/python3 "$1"
        echo "PASS: isolated fresh glibc-shim + 12 GPU cases + cached restart verification; host units/data unchanged"
    ' bash "$check"
