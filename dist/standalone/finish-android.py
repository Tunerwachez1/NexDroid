#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fixed first-boot operations. No arbitrary commands or user text accepted."""
import os
import pathlib
import pwd
import re
import subprocess
import sys
import time

def shell(*args, timeout=20):
    p = subprocess.run(['/usr/bin/waydroid', 'shell', '--', *args], check=True,
                       capture_output=True, text=True, timeout=timeout)
    return p.stdout.replace('\r', '').strip()

def finish(uid, keyboard_only=False):
    assert os.geteuid() == 0 and uid > 0
    deadline = time.monotonic() + (10 if keyboard_only else 180)
    while time.monotonic() < deadline:
        try:
            if shell('getprop', 'sys.boot_completed', timeout=max(0.1, min(5, deadline - time.monotonic()))) == '1':
                break
        except (subprocess.SubprocessError, OSError):
            pass
        time.sleep(1)
    else:
        raise SystemExit('Android first boot timed out; data retained')
    pkg = 'dev.waydroid.manager.home'
    row = shell('cmd', 'package', 'list', 'packages', '-U', pkg)
    match = re.fullmatch(r'package:dev\.waydroid\.manager\.home uid:(\d+)', row)
    assert match, 'Bundled Desktop APK was not registered by Android'
    android_uid = int(match.group(1))
    assert 10000 <= android_uid <= 99999
    # Open components without following user-controlled symlinks as root.
    target = pathlib.Path(pwd.getpwuid(uid).pw_dir) / '.local/share/waydroid/data/local/tmp/waydroid-hardware-keyboard'
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in target.parts[1:]:
            try:
                os.mkdir(part, 0o755, dir_fd=fd)
            except FileExistsError:
                pass
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = child
        os.fchown(fd, uid, android_uid)
        os.fchmod(fd, 0o710)
    finally:
        os.close(fd)
    if keyboard_only:
        return
    shell('ime', 'enable', pkg + '/.HardwareIme')
    shell('settings', 'put', 'secure', 'show_ime_with_hard_keyboard', '0')
    shell('cmd', 'package', 'set-home-activity', '--user', '0', pkg + '/.MainActivity')

if __name__ == '__main__':
    finish(int(sys.argv[1]), keyboard_only=sys.argv[2:] == ["--keyboard-only"])
