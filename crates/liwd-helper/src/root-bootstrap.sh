#!/system/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Container-only Magisk; no host kernel or phone boot image modifications.
set -eu
src=/system/etc/nexdroid-root/payload
data=/data/adb/magisk
runtime=/debug_ramdisk
mkdir -p "$data"
chmod 0700 /data/adb "$data"
for file in "$src"/*; do
    name=${file##*/}
    if [ ! -e "$data/$name" ]; then
        cp "$file" "$data/$name"
        case "$name" in *.apk) chmod 0600 "$data/$name";; *) chmod 0755 "$data/$name";; esac
    fi
done
if ! grep -q ' /debug_ramdisk ' /proc/mounts; then
    mount -t tmpfs -o mode=0755,nosuid magisk "$runtime"
fi
mkdir -p "$runtime/.magisk/device" "$runtime/.magisk/mirror" "$runtime/.magisk/rootdir" "$runtime/.magisk/worker"
chmod 0711 "$runtime/.magisk" "$runtime/.magisk/device"
if ! grep -q ' /debug_ramdisk/.magisk/worker ' /proc/mounts; then
    mount -t tmpfs -o mode=0755,nosuid magisk "$runtime/.magisk/worker"
fi
cp "$src/magisk" "$runtime/magisk"
cp "$src/stub.apk" "$runtime/stub.apk"
chmod 0755 "$runtime/magisk"
chmod 0644 "$runtime/stub.apk"
ln -sf magisk "$runtime/su"
ln -sf magisk "$runtime/resetprop"
printf 'RECOVERYMODE=false\nKEEPVERITY=true\nKEEPFORCEENCRYPT=true\n' > "$runtime/.magisk/config"
chmod 0600 "$runtime/.magisk/config"
"$runtime/magisk" --daemon
# Apps only, individual Magisk approvals; never insert wildcard allow policies.
"$runtime/magisk" --sqlite "INSERT OR REPLACE INTO settings (key,value) VALUES('root_access',1),('multiuser_mode',0),('mnt_ns',1);"
"$runtime/magisk" --sqlite "INSERT OR IGNORE INTO strings (key,value) VALUES('requester','@MANAGER@');"
"$runtime/magisk" --post-fs-data
