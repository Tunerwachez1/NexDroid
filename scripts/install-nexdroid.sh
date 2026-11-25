#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Arch/CachyOS beta installer: one entry point, no separate Waydroid download.
set -euo pipefail
repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
dry=false
case "${1:-}" in --dry-run) dry=true;; "") ;; *) echo 'Usage: bash scripts/install-nexdroid.sh [--dry-run]' >&2; exit 2;; esac
run() { if "$dry"; then printf 'RUN'; printf ' %q' "$@"; printf '\n'; else "$@"; fi; }
fail() { echo "$*" >&2; exit 1; }
[[ $EUID != 0 ]] || fail 'Run as the logged-in desktop user, without sudo.'
[[ $(uname -m) == x86_64 ]] || fail 'This beta installer supports x86_64 only.'
[[ -n ${WAYLAND_DISPLAY:-} && -n ${XDG_RUNTIME_DIR:-} ]] || fail 'Log into a Wayland desktop first.'
command -v pacman >/dev/null || fail 'Automatic installation currently supports Arch/CachyOS/Manjaro only. See README for source builds.'
[[ -d /run/systemd/system ]] || fail 'A systemd desktop is required.'
if ! "$dry" && pgrep -u "$(id -u)" -x liw-ui >/dev/null; then
    fail 'Close NexDroid before installation, then run the installer again.'
fi
apk="$repo/android/desktop/prebuilt/waydroid-desktop.apk"
[[ -f "$apk" && -f "$apk.sha256" ]] || fail 'The Android APK is missing from this checkout/release.'
(cd "$(dirname "$apk")" && sha256sum -c waydroid-desktop.apk.sha256)
bundle="$repo/dist/android"
if [[ ! -f /var/lib/waydroid/waydroid.cfg ]]; then
    # A full release ships OUR images. Never silently substitute stock OTA Android.
    for file in system.img vendor.img manifest.json SHA256SUMS; do
        [[ -f "$bundle/$file" && ! -L "$bundle/$file" ]] || fail 'Bundled NexDroid Android is missing. Use the full release archive; a source checkout alone does not contain Android images.'
    done
    (cd "$bundle"; sha256sum -c SHA256SUMS)
    python3 - "$bundle/manifest.json" <<'PY'
import json, sys
m = json.load(open(sys.argv[1]))
if not (m.get('format') == 1 and m.get('android') == 13 and m.get('arch') == 'x86_64'
        and m.get('userdata_included') is False):
    raise SystemExit('Unsupported NexDroid Android bundle')
PY
    [[ ! -e /etc/waydroid-extra/images && ! -e /usr/share/waydroid-extra/images ]] || fail 'Preinstalled Waydroid images already exist. Refusing to overwrite them.'
fi

echo 'NexDroid beta: install Waydroid, Android and the NexDroid desktop.'
echo 'Existing Android images and application data will be retained.'
run sudo pacman -S --needed waydroid wayland libxkbcommon libxkbcommon-x11 libxcb libx11 fontconfig freetype2 mesa vulkan-icd-loader libdrm libpulse alsa-lib dbus polkit systemd nftables dnsmasq procps-ng libinput python
if [[ ! -x "$repo/target/release/liw-ui" || ! -x "$repo/target/release/liwd" || ! -x "$repo/target/release/liw" || ! -x "$repo/target/release/liwd-helper" ]]; then
    run sudo pacman -S --needed base-devel git rust cargo clang cmake pkgconf
    (cd "$repo"; run cargo build --release --workspace)
fi
if ! "$dry"; then
    for binary in liw-ui liwd liw liwd-helper; do
        if ldd "$repo/target/release/$binary" | grep -q 'not found'; then fail "Missing runtime library for $binary. No Android changes made."; fi
    done
fi
if [[ ! -f /var/lib/waydroid/waydroid.cfg ]]; then
    run sudo install -d -m755 /usr/share/waydroid-extra/images
    for file in system.img vendor.img; do
        run sudo install -m644 "$bundle/$file" "/usr/share/waydroid-extra/images/$file"
    done
    # Waydroid recognizes this preinstalled-images path and skips OTA downloads.
    run sudo waydroid init -s GAPPS
else
    echo 'Keeping the existing Android installation (no init --force).'
fi

# Do not replace a host NVIDIA driver or bypass an exact-version GPU gate.
if [[ -f /proc/driver/nvidia/version ]]; then
    [[ -f /var/lib/waydroid-nvidia/vendor-image/install.json ]] || fail 'NVIDIA requires the native Waydroid backend first; see docs/unified-install.md. Android has been prepared, but GPU compatibility is not yet configured.'
    run sudo /usr/bin/python3 /usr/lib/waydroid-nvidia/native/dev/driver-mode verify-active
fi
run sudo bash "$repo/dist/install-helper.sh"
run sudo systemctl restart liwd-helper.service
for binary in liw liwd liw-ui; do run install -Dm755 "$repo/target/release/$binary" "$HOME/.local/bin/$binary"; done
run install -Dm644 "$repo/dist/systemd/liwd.service" "$HOME/.config/systemd/user/liwd.service"
config="${XDG_CONFIG_HOME:-$HOME/.config}/liwinux/config.toml"
if [[ ! -f "$config" ]]; then run install -Dm644 "$repo/dist/config/embedded.toml" "$config"; fi
run bash "$repo/dist/install-desktop.sh"
run systemctl --user daemon-reload
run sudo systemctl start waydroid-container.service
if ! "$dry"; then
    # Bootstrap on the host display only for initial provisioning. Refuse to
    # compete with a running NexDroid window; updating can be retried after closing it.
    if pgrep -u "$(id -u)" -x liw-ui >/dev/null; then
        fail 'Close NexDroid before provisioning its Android component, then run the installer again.'
    fi
fi
run setsid --fork waydroid session start
if ! "$dry"; then
    ready=false
    for ((attempt=0; attempt<120; attempt++)); do
        if [[ $(sudo waydroid shell -- getprop sys.boot_completed 2>/dev/null | tr -d '\r\n') == 1 ]]; then ready=true; break; fi
        sleep 1
    done
    "$ready" || fail 'Android did not finish booting. Check waydroid log; existing data has been retained.'
fi
if "$dry"; then
    run sudo waydroid shell -- pm install -r /data/local/tmp/nexdroid-install.UNIQUE/desktop.apk
else
    stage_dir=$(sudo mktemp -d "$HOME/.local/share/waydroid/data/local/tmp/nexdroid-install.XXXXXXXX")
    sudo chmod 755 "$stage_dir"
    sudo install -m644 "$apk" "$stage_dir/desktop.apk"
    result=$(sudo waydroid shell -- pm install -r "/data/local/tmp/$(basename "$stage_dir")/desktop.apk" | tr -d '\r')
    printf '%s\n' "$result"
    [[ $result == *Success* && $result != *Failure* ]] || fail 'Android APK installation failed; no IME/HOME changes made.'
fi
if ! "$dry"; then
    android_uid=$(sudo waydroid shell -- cmd package list packages -U dev.waydroid.manager.home | tr -d '\r' | sed -n 's/^package:dev\.waydroid\.manager\.home uid:\([0-9][0-9]*\)$/\1/p')
    [[ $android_uid =~ ^[0-9]+$ && $android_uid -ge 10000 && $android_uid -le 99999 ]] || fail 'Could not verify the installed Android desktop UID.'
else android_uid=10000; fi
run sudo install -d -m710 -o "$(id -u)" -g "$android_uid" "$HOME/.local/share/waydroid/data/local/tmp/waydroid-hardware-keyboard"
run sudo waydroid shell -- ime enable dev.waydroid.manager.home/.HardwareIme
run sudo waydroid shell -- ime set dev.waydroid.manager.home/.HardwareIme
run sudo waydroid shell -- settings put secure show_ime_with_hard_keyboard 0
run sudo waydroid shell -- cmd package set-home-activity --user 0 dev.waydroid.manager.home/.MainActivity
run install -Dm644 "$repo/android/desktop/host/layout_sync.py" "$HOME/.local/share/nexdroid/layout_sync.py"
run install -Dm644 "$repo/dist/systemd/nexdroid-keyboard.service" "$HOME/.config/systemd/user/nexdroid-keyboard.service"
# Earlier development installations used this name for the same socket.
if systemctl --user cat waydroid-layout-sync.service >/dev/null 2>&1; then
    run systemctl --user disable --now waydroid-layout-sync.service
fi
run systemctl --user daemon-reload
run systemctl --user enable --now nexdroid-keyboard.service
run waydroid session stop
if "$dry"; then echo 'Dry run complete; nothing was installed.'
else echo 'NexDroid installed. Open NexDroid Desktop from your application menu.'; fi
echo 'Bundled Android includes ARMv7/ARM64. NVIDIA still requires its exact-version native backend. Google Play may require normal registration/sign-in.'
