# NexDroid host-driver adapter v1. Appended to the bundled nvidia_native module.
# SPDX-License-Identifier: GPL-3.0-or-later
_nexdroid_original_read_ld_cache = read_ld_cache

def read_ld_cache(path=None):
    """Use the host's canonical driver paths inside the private runtime.

    Written by host-nvidia.py outside Bubblewrap; never resolve host symlinks
    through the bundled /usr. No driver library is copied into the release.
    """
    manifest = os.environ.get('NEXDROID_HOST_DRIVER_MANIFEST')
    mirror = os.environ.get('NEXDROID_HOST_DRIVER_ROOT')
    if path is not None or not manifest or not mirror:
        return _nexdroid_original_read_ld_cache(path)
    import json
    import stat
    info = os.stat(manifest, follow_symlinks=False)
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
        raise OSError('NexDroid: unsafe host NVIDIA manifest')
    with open(manifest) as stream:
        data = json.load(stream)
    if data.get('format') != 1 or data['driver'] != driver_version():
        raise OSError('NexDroid: host driver changed; prepare its libraries before starting Android')
    entries = []
    for flags, name, source in data['cache']:
        parts = source.split('/')
        if not source.startswith(('/usr/', '/lib/', '/lib64/')) or '..' in parts:
            raise OSError('NexDroid: unsupported host driver path: ' + source)
        entries.append((flags, name, os.path.join(mirror, source.lstrip('/'))))
    return entries
