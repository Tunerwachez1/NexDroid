#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Maintainer integration check: isolated fresh work directories, no host units.
set -euo pipefail
[[ $EUID == 0 && $# == 2 ]] || { echo 'Usage: sudo bash scripts/check-standalone-fresh.sh BUNDLE NEW_TEST_DIRECTORY'; exit 2; }
base=$(realpath "$1")
work=$(realpath -m "$2")
[[ -f "$base/payload-id" && ! -e $work ]] || exit 1
mkdir -p "$work/android" "$work/gpu"
root="$base/runtime"
# /opt is a tmpfs so creation of /opt/nexdroid/current cannot change the host.
# Only the two test work trees receive generated configs, roots and gate logs.
exec "$root/usr/lib/ld-linux-x86-64.so.2" --library-path "$root/usr/lib" \
    "$root/usr/bin/bwrap" --bind / / --dev-bind /dev /dev --cap-add ALL --ro-bind "$root/usr" /usr \
    --ro-bind-try /usr/lib/modules /usr/lib/modules \
    --ro-bind-try /usr/lib/firmware /usr/lib/firmware \
    --ro-bind-try /usr/bin/nvidia-modprobe /usr/bin/nvidia-modprobe \
    --tmpfs /opt --ro-bind "$base" /opt/nexdroid/current \
    --bind "$work/android" /var/lib/waydroid \
    --bind "$work/gpu" /var/lib/waydroid-nvidia \
    --setenv PATH /usr/bin:/bin --setenv PYTHONNOUSERSITE 1 --setenv PYTHONDONTWRITEBYTECODE 1 \
    --unsetenv PYTHONPATH --unsetenv PYTHONHOME --unsetenv LD_PRELOAD \
    --unsetenv PYTHONOPTIMIZE \
    -- /usr/bin/bash -ec '
        ln -s /opt/nexdroid/current/images /var/lib/waydroid/images
        waydroid init -i /opt/nexdroid/current/images -s GAPPS
        python3 /opt/nexdroid/current/provision-gpu.py
        python3 /opt/nexdroid/current/startup-check.py
        echo "PASS: isolated fresh init + full GPU provisioning; no host units/data changed"
    '
