#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
import hashlib
import json
import pathlib
import sys

def verify(root):
    data = (root / 'payload.json').read_bytes()
    assert hashlib.sha256(data).hexdigest() == (root / 'payload-id').read_text().strip(), 'Payload manifest changed'
    manifest = json.loads(data)
    assert manifest['format'] == 1
    actual = {str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() or p.is_symlink()}
    assert actual == set(manifest['files']) | {'payload.json', 'payload-id'}, 'Unexpected/missing payload files'
    for rel, entry in manifest['files'].items():
        path = pathlib.PurePosixPath(rel)
        assert not path.is_absolute() and '..' not in path.parts, rel
        p = root / rel
        if 'symlink' in entry:
            assert p.is_symlink() and str(p.readlink()) == entry['symlink'], rel
        else:
            assert p.is_file() and not p.is_symlink(), rel
            with p.open('rb') as f:
                assert hashlib.file_digest(f, 'sha256').hexdigest() == entry['sha256'], rel
            assert p.stat().st_mode & 0o111 == entry['exec'], rel
    print('PASS: standalone payload hashes, symlinks and executable modes')

if __name__ == '__main__':
    verify(pathlib.Path(sys.argv[1]).resolve())
