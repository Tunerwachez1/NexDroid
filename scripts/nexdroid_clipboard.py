# SPDX-License-Identifier: GPL-3.0-or-later
"""Desktop-independent clipboard backend for NexDroid."""

import os
import selectors
import subprocess
import time

MAX_BYTES = 256 * 1024
TIMEOUT = 3


def enabled():
    return os.environ.get("NEXDROID_CLIPBOARD") != "0"


def backend():
    env = os.environ.copy()

    if not enabled():
        raise RuntimeError("Clipboard disabled")

    if "NEXDROID_HOST_WAYLAND_DISPLAY" in env:
        env["WAYLAND_DISPLAY"] = env["NEXDROID_HOST_WAYLAND_DISPLAY"]
        env.pop("WAYLAND_SOCKET", None)

    if env.get("WAYLAND_DISPLAY"):
        return (
            ["wl-copy", "--type", "text/plain;charset=utf-8"],
            ["wl-paste", "--no-newline", "--type", "text"],
            env,
        )

    if env.get("DISPLAY") and env.get("XDG_SESSION_TYPE") != "wayland":
        return (
            ["xclip", "-selection", "clipboard", "-in", "-target", "UTF8_STRING"],
            ["xclip", "-selection", "clipboard", "-out", "-target", "UTF8_STRING"],
            env,
        )

    raise RuntimeError("No supported clipboard backend")


def copy(value):
    data = value.encode("utf-8")

    if len(data) > MAX_BYTES:
        raise ValueError("Clipboard text too large")

    command, _, env = backend()

    subprocess.run(
        command,
        input=data,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=env,
        timeout=TIMEOUT,
        check=True,
    )


def paste():
    _, command, env = backend()

    deadline = time.monotonic() + TIMEOUT
    with subprocess.Popen(
        command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, env=env,
    ) as process, selectors.DefaultSelector() as selector:
        try:
            selector.register(process.stdout, selectors.EVENT_READ)
            data = bytearray()
            while len(data) <= MAX_BYTES:
                if not selector.select(max(0, deadline - time.monotonic())):
                    raise TimeoutError("Clipboard read timed out")
                chunk = os.read(process.stdout.fileno(), min(16384, MAX_BYTES + 1 - len(data)))
                if not chunk:
                    if process.wait(timeout=max(0, deadline - time.monotonic())):
                        raise RuntimeError("Clipboard backend failed")
                    return data.decode("utf-8")
                data.extend(chunk)
            raise ValueError("Clipboard text too large")
        finally:
            if process.poll() is None:
                process.kill()
