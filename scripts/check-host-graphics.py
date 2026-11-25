#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Render/read one GLES pixel on a headless NVIDIA EGL device, inside the runtime."""
import ctypes as c
import json
import os
import subprocess
import sys

I, P, U = c.c_int, c.c_void_p, c.c_uint
egl = c.CDLL('libEGL.so.1')
egl.eglGetProcAddress.argtypes = [c.c_char_p]
egl.eglGetProcAddress.restype = P

def ext(name, restype, *args):
    addr = egl.eglGetProcAddress(name.encode())
    if not addr: raise RuntimeError('Missing EGL extension: ' + name)
    return c.CFUNCTYPE(restype, *args)(addr)

def core(name, restype, *args):
    fn = getattr(egl, name)
    fn.restype, fn.argtypes = restype, list(args)
    return fn

devices = (P * 16)()
count = I()
assert ext('eglQueryDevicesEXT', U, I, c.POINTER(P), c.POINTER(I))(16, devices, c.byref(count))
assert count.value, 'No EGL GPU device'
initialize = core('eglInitialize', U, P, c.POINTER(I), c.POINTER(I))
query = core('eglQueryString', c.c_char_p, P, I)
terminate = core('eglTerminate', U, P)
display = None
for device in devices[:count.value]:
    candidate = ext('eglGetPlatformDisplayEXT', P, U, P, c.POINTER(I))(0x313F, device, None)
    if initialize(candidate, None, None):
        if b'NVIDIA' in query(candidate, 0x3053):
            display = candidate
            break
        terminate(candidate)
assert display, 'No initialized NVIDIA EGL display'
try:
    assert core('eglBindAPI', U, U)(0x30A0)
    attrs = (I * 11)(0x3033, 1, 0x3040, 4, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3038)
    config, found = P(), I()
    assert core('eglChooseConfig', U, P, c.POINTER(I), c.POINTER(P), I, c.POINTER(I))(
        display, attrs, c.byref(config), 1, c.byref(found)) and found.value
    surface = core('eglCreatePbufferSurface', P, P, P, c.POINTER(I))(
        display, config, (I * 5)(0x3057, 1, 0x3056, 1, 0x3038))
    context = core('eglCreateContext', P, P, P, P, c.POINTER(I))(
        display, config, None, (I * 3)(0x3098, 2, 0x3038))
    assert surface and context
    assert core('eglMakeCurrent', U, P, P, P, P)(display, surface, surface, context)
    gl = c.CDLL('libGLESv2.so.2')
    gl.glGetString.restype, gl.glGetString.argtypes = c.c_char_p, [U]
    renderer = gl.glGetString(0x1F01).decode()
    assert 'NVIDIA' in renderer or 'GeForce' in renderer, renderer
    gl.glClearColor.argtypes = [c.c_float] * 4
    gl.glClear.argtypes = [U]
    gl.glReadPixels.argtypes = [I, I, I, I, U, U, P]
    gl.glClearColor(0.25, 0.5, 0.75, 1.0)
    gl.glClear(0x4000)
    pixel = (c.c_ubyte * 4)()
    gl.glReadPixels(0, 0, 1, 1, 0x1908, 0x1401, pixel)
    assert all(abs(a - b) <= 1 for a, b in zip(pixel, (64, 128, 191, 255))), list(pixel)
    maps = [line.strip() for line in open('/proc/self/maps') if 'libEGL_nvidia' in line]
    assert maps and all('/runtime/usr/lib/' not in line for line in maps), maps
    print(json.dumps({'result': 'PASS', 'renderer': renderer, 'pixel': list(pixel), 'driver_maps': maps}))
finally:
    terminate(display)

# Exercise the same nested entry as GPUI's graphics re-exec and daemon calls.
if '--nested' not in sys.argv:
    base = os.environ['NEXDROID_PRIVATE_RUNTIME']
    subprocess.run([base + '/runtime-exec', sys.executable, __file__, '--nested'], check=True)
