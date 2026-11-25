#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Prepare ONE shared host Binder context before any private runtime mounts."""
import configparser
import errno
import fcntl
import os
import pathlib
import re
import stat
import struct
import subprocess

if os.geteuid() != 0:
    raise SystemExit('Binder device preparation requires the system service')
cfg = configparser.ConfigParser()
cfg.read('/var/lib/waydroid/waydroid.cfg')
names = [cfg.get('waydroid', key, fallback=key) for key in ('binder', 'vndbinder', 'hwbinder')]
if not all(re.fullmatch(r'[a-zA-Z0-9_-]{1,63}', name) for name in names):
    raise SystemExit('Invalid Binder device names in configuration')

def char(path):
    try:
        return stat.S_ISCHR(os.stat(path).st_mode)
    except FileNotFoundError:
        return False

if not all(char('/dev/' + n) for n in names):
    if not any(line.split()[-1] == 'binder' for line in pathlib.Path('/proc/filesystems').read_text().splitlines()):
        subprocess.run(['/usr/bin/modprobe', 'binder_linux'], check=True)
    directory = pathlib.Path('/dev/binderfs')
    directory.mkdir(exist_ok=True)
    mounts = [line.split() for line in pathlib.Path('/proc/self/mountinfo').read_text().splitlines()]
    binder = any(row[4] == '/dev/binderfs' and row[row.index('-') + 1] == 'binder' for row in mounts)
    if not binder:
        if os.path.ismount(directory):
            raise SystemExit('/dev/binderfs is mounted with another filesystem; refusing to replace it')
        subprocess.run(['/usr/bin/mount', '-t', 'binder', 'binder', str(directory)], check=True)
    with (directory / 'binder-control').open('rb') as control:
        for name in names:
            if char('/dev/' + name):
                continue
            try:
                fcntl.ioctl(control.fileno(), (3 << 30) | (264 << 16) | (98 << 8) | 1,
                            struct.pack('256sII', name.encode(), 0, 0))
            except OSError as e:
                if e.errno != errno.EEXIST:
                    raise
            node = pathlib.Path('/dev') / name
            target = directory / name
            if node.is_symlink():
                if os.readlink(node) != str(target):
                    raise SystemExit(f'Unexpected device symlink: {node}')
            elif node.exists():
                raise SystemExit(f'Unexpected existing device: {node}')
            else:
                node.symlink_to(target)
if not all(char('/dev/' + n) for n in names):
    raise SystemExit('Binder devices were not created')
print('Shared host Binder devices ready:', ', '.join(names))
