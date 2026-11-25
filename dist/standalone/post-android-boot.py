#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fixed standalone guest maintenance, started by the container service."""

import os
import pathlib
import pwd
import re
import subprocess
import sys
import time

DESKTOP = "dev.waydroid.manager.home"
GOOGLE = "com.google.android.googlequicksearchbox"
ASSISTANT = GOOGLE + (
    "/com.google.android.apps.search.assistant.surfaces.voice.ui.host"
    ".activity.defaultactivity.FragmentHostDefaultActivity"
)
ASSISTANT_POLICY = "nexdroid_assistant_policy"


def android(*args, timeout=8):
    return subprocess.run(
        ["/usr/bin/lxc-attach", "-P", "/var/lib/waydroid/lxc", "-n", "waydroid",
         "--", *args],
        stdin=subprocess.DEVNULL, capture_output=True, text=True, check=True, timeout=timeout,
    ).stdout.strip()


def wait_for_boot():
    deadline = time.monotonic() + 120
    while (remaining := deadline - time.monotonic()) > 0:
        try:
            if android("/system/bin/getprop", "sys.boot_completed", timeout=min(5, remaining)) == "1":
                return True
        except (OSError, subprocess.SubprocessError):
            pass
        time.sleep(1)
    return False


def repair_keyboard(uid, android_uid):
    # Only fixed paths below the account's passwd home. Never create or
    # traverse intermediate symlinks, and never use a path from the D-Bus call.
    home = pathlib.Path(pwd.getpwuid(uid).pw_dir)
    parent = home / ".local/share/waydroid/data/local/tmp"
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    fd = os.open("/", flags)
    try:
        for part in parent.parts[1:]:
            child = os.open(part, flags, dir_fd=fd)
            os.close(fd)
            fd = child
        if os.fstat(fd).st_uid != 2000:
            raise PermissionError("Unexpected owner of Android /data/local/tmp")
        name = "waydroid-hardware-keyboard"
        try:
            os.mkdir(name, 0o700, dir_fd=fd)
        except FileExistsError:
            pass
        child = os.open(name, flags, dir_fd=fd)
        try:
            if os.fstat(child).st_uid not in (0, uid):
                raise PermissionError("Unexpected owner of keyboard directory")
            os.fchown(child, uid, android_uid)
            os.fchmod(child, 0o710)
        finally:
            os.close(child)
    finally:
        os.close(fd)


def configure_keyboard(uid):
    row = android("/system/bin/cmd", "package", "list", "packages", "-U", "--user", "0", DESKTOP)
    match = re.fullmatch(r"package:dev\.waydroid\.manager\.home uid:([1-9][0-9]{4})", row)
    if match is None:
        print("NexDroid: Android image lacks the Desktop APK for the keyboard bridge", file=sys.stderr)
        return
    repair_keyboard(uid, int(match.group(1)))


def configure_assistant():
    if android("/system/bin/settings", "--user", "0", "get", "secure", ASSISTANT_POLICY) == "1":
        return
    # The observed Google voice UI is self-launched even with no role holder.
    # Apply the default once so a later explicit user change can persist.
    if not android("/system/bin/cmd", "package", "list", "packages", "--user", "0", GOOGLE).splitlines().count(f"package:{GOOGLE}"):
        return
    result = android("/system/bin/pm", "disable", "--user", "0", ASSISTANT)
    if not result.endswith("new state: disabled"):
        raise RuntimeError("Could not apply Android Assistant default")
    android("/system/bin/settings", "--user", "0", "put", "secure", ASSISTANT_POLICY, "1")


def main(uid):
    if os.geteuid() != 0 or uid <= 0:
        raise PermissionError("Android maintenance requires a verified desktop UID")
    if not wait_for_boot():
        print("NexDroid: Android did not boot before the maintenance deadline", file=sys.stderr)
        return
    for label, operation in (
        ("keyboard directory", lambda: configure_keyboard(uid)),
        ("Assistant default", configure_assistant),
    ):
        try:
            operation()
        except (OSError, RuntimeError, subprocess.SubprocessError) as error:
            print(f"NexDroid: {label} failed ({type(error).__name__})", file=sys.stderr)


if __name__ == "__main__":
    main(int(sys.argv[1]))
