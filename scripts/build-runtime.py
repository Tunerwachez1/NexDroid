#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Maintainer: assemble a private /usr from the installed Arch package database.

No package manager runs on the end user's machine. No /etc, home or userdata
is collected. Installed package licenses and a version inventory are retained.
"""
import json
import os
import pathlib
import re
import shutil
import sys

def sections(path):
    out, key = {}, None
    for line in path.read_text().splitlines():
        if line.startswith('%') and line.endswith('%'):
            key = line.strip('%'); out[key] = []
        elif line and key:
            out[key].append(line)
    return out

def name(expr):
    return re.split(r'[<>=]', expr, maxsplit=1)[0]

def patch_host_driver(out):
    """Preserve host NVIDIA discovery when /usr is replaced by the private runtime."""
    adapter = (pathlib.Path(__file__).resolve().parents[1] /
               'dist/standalone/host-cache-adapter.py').read_text()
    marker = '\n# NexDroid host-driver adapter v1.'
    for rel in ['usr/lib/waydroid/tools/helpers/nvidia_native.py',
                'usr/lib/waydroid-nvidia/native/waydroid/nvidia_native.py']:
        p = out / rel
        text = p.read_text()
        end = '\n# End NexDroid host-driver adapter v1.'
        if marker in text:
            before, rest = text.split(marker, 1)
            text = before + (rest.split(end, 1)[1] if end in rest else '')
        boundary = text.rfind('\nif __name__ == "__main__":')
        if boundary < 0:
            raise RuntimeError('Review NVIDIA module entry point before patching: ' + str(p))
        p.write_text(text[:boundary].rstrip() + '\n\n' + adapter.rstrip() + end + '\n' + text[boundary:])
    for name in ('usr', 'lib', 'lib64'):
        (out / 'host' / name).mkdir(parents=True, exist_ok=True)
    # Never ship this builder's proprietary userspace as a runtime fallback.
    # GLVND + generic Mesa stay bundled; NVIDIA comes from the user's host.
    for sub in ('usr/lib', 'usr/lib32'):
        for pattern in ('libnvidia-*', 'libEGL_nvidia*', 'libGLX_nvidia*',
                        'libcuda.so*', 'libnvcuvid.so*', 'libnvoptix.so*'):
            for path in (out / sub).glob(pattern):
                if path.is_file() or path.is_symlink(): path.unlink()

    vendors = out / 'usr/share/glvnd/egl_vendor.d'
    vendors.mkdir(parents=True, exist_ok=True)
    for name, library in [('10_nvidia.json', 'libEGL_nvidia.so.0'),
                          ('50_mesa.json', 'libEGL_mesa.so.0')]:
        (vendors / name).write_text(json.dumps({'file_format_version': '1.0.0',
            'ICD': {'library_path': library}}) + '\n')
    platforms = out / 'usr/share/egl/egl_external_platform.d'
    platforms.mkdir(parents=True, exist_ok=True)
    for index, name in enumerate(('wayland', 'wayland2', 'gbm', 'xcb', 'xlib')):
        (platforms / f'{10 + index}_nvidia_{name}.json').write_text(json.dumps({
            'file_format_version': '1.0.0', 'ICD': {
                'library_path': f'libnvidia-egl-{name}.so.1'}}) + '\n')

def patch_clipboard(out):
    services = out / "usr/lib/waydroid/tools/services"
    manager = services / "clipboard_manager.py"
    adapter = pathlib.Path(__file__).resolve().parent / "nexdroid_clipboard.py"

    for name in ("wl-copy", "wl-paste", "xclip"):
        if not (out / "usr/bin" / name).is_file():
            raise RuntimeError("Missing clipboard dependency: " + name)

    shutil.copyfile(adapter, services / "nexdroid_clipboard.py")

    text = manager.read_text()
    original = "    import pyclip"
    replacement = "    from tools.services import nexdroid_clipboard as pyclip"

    if replacement not in text:
        if text.count(original) != 1:
            raise RuntimeError("Unsupported Waydroid clipboard manager layout")
        text = text.replace(original, replacement, 1)

    if "    canClip = pyclip.enabled()" not in text:
        if text.count("    canClip = True") != 1:
            raise RuntimeError("Unsupported Waydroid clipboard manager feature check")
        text = text.replace("    canClip = True", "    canClip = pyclip.enabled()", 1)
    manager.write_text(text)


def patch_post_android_boot(out):
    actions = out / "usr/lib/waydroid/tools/actions"
    manager = actions / "container_manager.py"
    standalone = pathlib.Path(__file__).resolve().parent.parent / "dist/standalone"
    shutil.copyfile(standalone / "post-android-boot.py", actions / "nexdroid_post_boot.py")

    text = manager.read_text()
    start = "        do_start(self.args, session)"
    stop = "    logging.info(\"Stopping container\")"
    if "self.args.nexdroid_post_boot = Gio.Subprocess.new" in text:
        return
    if text.count(start) != 1 or text.count(stop) != 1 or text.count("from gi.repository import GLib") != 1:
        raise RuntimeError("Unsupported Waydroid container manager layout")
    text = text.replace("from gi.repository import GLib", "from gi.repository import Gio, GLib")
    text = text.replace(start, start + """
        if int(uid) > 0:
            try:
                self.args.nexdroid_post_boot = Gio.Subprocess.new(
                    ["/usr/bin/python3", os.path.join(os.path.dirname(__file__), "nexdroid_post_boot.py"), str(uid)],
                    Gio.SubprocessFlags.NONE,
                )
            except GLib.Error:
                logging.warning("Could not start NexDroid post-boot maintenance")""")
    text = text.replace(stop, stop + """
    worker = getattr(args, "nexdroid_post_boot", None)
    if worker is not None:
        worker.force_exit()
        del args.nexdroid_post_boot
""")
    manager.write_text(text)


def finish(out):
    patch_clipboard(out)
    patch_post_android_boot(out)
    patch_host_driver(out)
    (out / 'usr/lib/waydroid/waydroid.py').chmod(0o755)
    config = out / 'usr/lib/waydroid/tools/config/__init__.py'
    text = config.read_text()
    old = '"preinstalled_images_paths": ['
    assert text.count(old) == 1
    text = text.replace(old, old + '\n        "/opt/nexdroid/current/images",')
    config.write_text(text)
    # Keep upstream's GPU decode test, but ship its executable instead of
    # compiling C++ on an end user's computer. LD_LIBRARY_PATH relocates it.
    gate = out / 'usr/lib/waydroid-nvidia/native/packaging/native/release_gate.py'
    text = gate.read_text()
    old = """step('decode-smoke', 'make', '-B', '-C', REPO / 'nvidia-prolapse/media', 'smoke',
         f'NV_VIDEO_LIB=-L{host_rust.parent} -lnv_bionic_native -Wl,-rpath,{host_rust.parent}',
         'CODECS=' + ' '.join(smoke))"""
    new = """step('decode-smoke', 'env', f'LD_LIBRARY_PATH={host_rust.parent}',
         'CODECS=' + ' '.join(smoke), 'bash', REPO / 'nvidia-prolapse/media/test/decode-smoke.sh')"""
    assert text.count(old) == 1, 'Unexpected media gate source; review before packaging'
    gate.write_text(text.replace(old, new))
    for path in ['usr/share/fonts', 'usr/share/icons', 'usr/lib/modules', 'usr/lib/firmware']:
        (out / path).mkdir(parents=True, exist_ok=True)
    (out / 'usr/bin/nvidia-modprobe').touch()

def build(out):
    if os.geteuid() != 0:
        raise SystemExit('Maintainer export requires sudo to read package runtime files')
    if out.exists():
        raise SystemExit('Output must not exist')
    packages, providers = {}, {}
    for desc in pathlib.Path('/var/lib/pacman/local').glob('*/desc'):
        meta = sections(desc)
        n = meta['NAME'][0]
        packages[n] = (desc.parent, meta)
        providers[n] = n
    for n, (_, meta) in packages.items():
        for expr in meta.get('PROVIDES', []):
            providers.setdefault(name(expr), n)
    selected, pending = set(), ['waydroid', 'bubblewrap', 'coreutils', 'util-linux',
        'iproute2', 'procps-ng', 'kmod', 'e2fsprogs', 'systemd', 'dbus', 'polkit', 'zenity', 'binutils',
        'gawk', 'grep', 'sed', 'findutils', 'diffutils', 'which', 'psmisc', 'wl-clipboard', 'xclip']
    while pending:
        expr = pending.pop()
        n = providers.get(name(expr))
        if n is None:
            raise SystemExit(f'Installed dependency is missing: {expr}')
        if n in selected:
            continue
        selected.add(n)
        pending.extend(packages[n][1].get('DEPENDS', []))
    out.mkdir(parents=True)
    copied = set()
    def copy(src):
        src = pathlib.Path(os.path.normpath(str(src)))
        # Arch usr-merge aliases used by packaged symlinks.
        for alias, target in [('/bin/', '/usr/bin/'), ('/sbin/', '/usr/bin/'),
                              ('/lib/', '/usr/lib/'), ('/lib64/', '/usr/lib/')]:
            if str(src).startswith(alias):
                src = pathlib.Path(target + str(src)[len(alias):])
                break
        if src in copied:
            return
        if not str(src).startswith('/usr/'):
            raise SystemExit(f'Package symlink escapes /usr: {src}')
        copied.add(src)
        for parent in reversed(src.parents):
            if str(parent).startswith('/usr/') and parent.is_symlink():
                copy(parent)
        dst = out / src.relative_to('/')
        if src.is_symlink():
            dst.parent.mkdir(parents=True, exist_ok=True)
            target = os.readlink(src)
            resolved = pathlib.Path(target) if target.startswith('/') else src.parent / target
            resolved = pathlib.Path(os.path.normpath(str(resolved)))
            if str(resolved).startswith('/usr/'):
                dst.symlink_to(os.path.relpath(out / resolved.relative_to('/'), dst.parent))
            else:
                # Host configuration/runtime references stay external; never export them.
                if not str(resolved).startswith(('/etc/', '/run/', '/proc/', '/dev/')):
                    raise SystemExit(f'Unsupported external symlink: {src} -> {resolved}')
                dst.symlink_to(str(resolved))
            if str(resolved).startswith('/usr/') and (resolved.exists() or resolved.is_symlink()):
                copy(resolved)
        elif src.is_dir():
            dst.mkdir(parents=True, exist_ok=True)
        elif src.is_file():
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(src, dst)
            dst.chmod((src.stat().st_mode & 0o777) | 0o444)  # readable; no setuid/setgid
        else:
            raise SystemExit(f'Package file is missing or unsupported: {src}')
    paths = []
    for n in sorted(selected):
        for rel in sections(packages[n][0] / 'files').get('FILES', []):
            if rel.startswith(('usr/bin/', 'usr/lib/', 'usr/lib64/',
                'usr/share/lxc/', 'usr/share/waydroid', 'usr/share/gir-1.0/',
                'usr/share/glib-2.0/', 'usr/share/gtk-3.0/', 'usr/share/licenses/')):
                paths.append(pathlib.Path('/') / rel.rstrip('/'))
    # Directory aliases must precede children, even when another package owns them.
    for src in sorted((p for p in paths if p.is_symlink()), key=lambda p:len(p.parts)):
        copy(src)
    for src in paths:
        copy(src)
    # The local patched Waydroid tree owns NVIDIA integration. Keep it private.
    patched = pathlib.Path('/opt/waydroid-nvidia-manager/waydroid')
    if not patched.is_dir():
        raise SystemExit('Prepared NVIDIA-aware Waydroid source tree is missing')
    dest = out / 'usr/lib/waydroid'
    shutil.rmtree(dest)
    shutil.copytree(patched, dest, ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
    if pathlib.Path('/usr/lib/waydroid-nvidia').is_dir():
        shutil.copytree('/usr/lib/waydroid-nvidia', out / 'usr/lib/waydroid-nvidia',
                        symlinks=True, ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
    for filename in ['THIRD_PARTY.md', 'NVIDIA-INTEGRATION-LICENSE']:
        shutil.copyfile(patched.parent / filename, out / filename)
    # Package symlink may have pointed at the replaced tree; use a relative link.
    entry = out / 'usr/bin/waydroid'
    entry.unlink(missing_ok=True)
    entry.symlink_to('../lib/waydroid/waydroid.py')
    finish(out)
    inventory = {n: packages[n][1]['VERSION'][0] for n in sorted(selected)}
    (out / 'packages.json').write_text(json.dumps(inventory, indent=2) + '\n')
    print(f'Private runtime: {len(selected)} packages, {len(copied)} package paths')

if __name__ == '__main__':
    build(pathlib.Path(sys.argv[1]).resolve())
