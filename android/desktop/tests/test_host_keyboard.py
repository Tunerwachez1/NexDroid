# SPDX-License-Identifier: GPL-3.0-or-later
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('keyboard', Path(__file__).parents[1] / 'host/layout_sync.py')
keyboard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(keyboard)


class HostKeyboardTest(unittest.TestCase):
    def test_liwinux_state_is_only_read_from_running_owner(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'XDG_RUNTIME_DIR': directory}):
            root = Path(directory)
            state = root / 'liwinux-keyboard.json'
            state.write_text('{"layout":"ru","caps":true}')
            with (root / 'liwinux-ui.lock').open('wb') as owner:
                self.assertIsNone(keyboard.read_liwinux_state())
                keyboard.fcntl.flock(owner, keyboard.fcntl.LOCK_EX | keyboard.fcntl.LOCK_NB)
                self.assertEqual(keyboard.read_liwinux_state(), {'layout': 'ru', 'caps': True})
                state.write_text('{"layout":"us","caps":false}')
                self.assertEqual(keyboard.read_liwinux_state(), {'layout': 'us', 'caps': False})
                state.unlink()
                self.assertIsNone(keyboard.read_liwinux_state())

    def test_socket_is_discovered_without_login_environment(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'XDG_RUNTIME_DIR': directory}):
            os.environ.pop('WAYLAND_DISPLAY', None)
            folder = Path(directory) / 'driftwm'
            folder.mkdir()
            path = folder / 'ipc-wayland-1.sock'
            with keyboard.socket.socket(keyboard.socket.AF_UNIX, keyboard.socket.SOCK_STREAM) as server:
                server.bind(str(path))
                self.assertEqual(keyboard.wm_socket_path(), str(path))
                os.environ['WAYLAND_DISPLAY'] = 'wayland-0'
                self.assertEqual(keyboard.wm_socket_path(), str(path))

    def test_ambiguous_session_requires_matching_display(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'XDG_RUNTIME_DIR': directory}):
            os.environ.pop('WAYLAND_DISPLAY', None)
            folder = Path(directory) / 'driftwm'
            folder.mkdir()
            with keyboard.socket.socket(keyboard.socket.AF_UNIX, keyboard.socket.SOCK_STREAM) as first, \
                    keyboard.socket.socket(keyboard.socket.AF_UNIX, keyboard.socket.SOCK_STREAM) as second:
                first.bind(str(folder / 'ipc-wayland-0.sock'))
                second.bind(str(folder / 'ipc-wayland-1.sock'))
                with self.assertRaises(OSError):
                    keyboard.wm_socket_path()
                os.environ['WAYLAND_DISPLAY'] = 'wayland-1'
                self.assertEqual(keyboard.wm_socket_path(), str(folder / 'ipc-wayland-1.sock'))

    def test_each_read_observes_latest_layout_and_caps(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(keyboard.socket, 'socket') as connect:
            led = Path(directory) / 'brightness'
            wm = connect.return_value.__enter__.return_value
            for layout, caps in [('us', False), ('ru', False), ('ru', True), ('us', True), ('us', False)]:
                led.write_text('1' if caps else '0')
                wm.makefile.return_value = io.BytesIO(json.dumps({'Ok': {'State': {'layout_short': layout}}}).encode())
                self.assertEqual(keyboard.read_keyboard_state('/wm.sock', [led]), {'layout': layout, 'caps': caps})
            self.assertEqual(wm.sendall.call_count, 5)
            wm.sendall.assert_called_with(b'"State"\n')

    def test_missing_caps_is_not_treated_as_off(self):
        with patch.object(keyboard.socket, 'socket') as connect:
            connect.return_value.__enter__.return_value.makefile.return_value = io.BytesIO(b'{"Ok":{"State":{"layout_short":"ru"}}}')
            with self.assertRaises(ValueError):
                keyboard.read_keyboard_state('/wm.sock', [])

    def test_unknown_layout_is_not_replaced_with_old_state(self):
        with patch.object(keyboard.socket, 'socket') as connect:
            connect.return_value.__enter__.return_value.makefile.return_value = io.BytesIO(b'{"Ok":{"State":{"layout_short":"unknown"}}}')
            with self.assertRaises(ValueError):
                keyboard.read_keyboard_state('/wm.sock', [])
