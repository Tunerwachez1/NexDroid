# SPDX-License-Identifier: GPL-3.0-or-later
"""Serve current layout/Caps to our Android UID. No typed keys or text are read."""
import json
import os
from pathlib import Path
import socket
import stat
import struct
import fcntl


def read_liwinux_state():
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}'))
    try:
        with (runtime / 'liwinux-ui.lock').open('rb') as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_SH | fcntl.LOCK_NB)
            except BlockingIOError:
                # The Rust UI holds the exclusive lock for its lifetime.
                state = json.loads((runtime / 'liwinux-keyboard.json').read_text())
                if state['layout'] not in ('us', 'ru') or type(state['caps']) is not bool:
                    raise ValueError('Invalid Liwinux keyboard state')
                return {'layout': state['layout'], 'caps': state['caps']}
    except FileNotFoundError:
        pass
    return None


def wm_socket_path():
    directory = Path(os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}')) / 'driftwm'
    sockets = []
    for path in directory.glob('ipc-*.sock'):
        try:
            info = path.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISSOCK(info.st_mode) and info.st_uid == os.getuid():
            sockets.append(path)
    display = os.environ.get('WAYLAND_DISPLAY')
    preferred = directory / f'ipc-{display}.sock' if display else None
    if preferred in sockets:
        return str(preferred)
    if len(sockets) == 1:
        return str(sockets[0])
    raise OSError('No unique current driftwm socket')


def read_keyboard_state(wm_path=None, caps_paths=None):
    wm_path = wm_path or wm_socket_path()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as wm:
        wm.settimeout(0.2)
        wm.connect(wm_path)
        wm.sendall(b'"State"\n')
        with wm.makefile('rb') as response:
            layout = json.loads(response.readline(65536))['Ok']['State']['layout_short']
    if layout not in ('us', 'ru'):
        raise ValueError('Unsupported layout')
    paths = caps_paths if caps_paths is not None else Path('/sys/class/leds').glob('*::capslock/brightness')
    values = [path.read_text().strip() for path in paths]
    if not values or any(value not in ('0', '1') for value in values):
        raise ValueError('Caps Lock state unavailable')
    return {'layout': layout, 'caps': '1' in values}


def main():
    directory = Path.home() / '.local/share/waydroid/data/local/tmp/waydroid-hardware-keyboard'
    info = directory.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_gid < 10000 or info.st_mode & 7:
        raise SystemExit('Keyboard directory needs owner=host user, group=Android app UID, mode=0710')
    path = directory / 'state.sock'
    if path.exists():
        previous = path.lstat()
        if not stat.S_ISSOCK(previous.st_mode) or previous.st_uid != os.getuid():
            raise SystemExit('Refusing to replace unrelated socket path')
        path.unlink()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
        server.bind(str(path))
        path.chmod(0o666)  # Parent directory limits traversal to the app's group.
        server.listen(8)
        print('Current keyboard state socket ready', flush=True)
        while True:
            client, _ = server.accept()
            with client:
                client.settimeout(0.4)
                _, uid, _ = struct.unpack('3i', client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                if uid not in (os.getuid(), info.st_gid):
                    continue
                try:
                    state = read_liwinux_state() or read_keyboard_state()
                except (OSError, ValueError, KeyError, TypeError):
                    state = {'error': 'keyboard-state-unavailable'}
                try:
                    client.sendall((json.dumps(state) + '\n').encode())
                except OSError:
                    pass


if __name__ == '__main__':
    main()
