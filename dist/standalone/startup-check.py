#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
import configparser
import subprocess
import sys

cfg = configparser.ConfigParser()
cfg.read('/var/lib/waydroid/waydroid.cfg')
if cfg.get('waydroid', 'nvidia_driver_mode', fallback=''):
    subprocess.run([sys.executable, '/usr/lib/waydroid-nvidia/native/dev/driver-mode', 'verify-active'], check=True)
