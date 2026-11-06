#!/usr/bin/env python3
"""Publish signed code via a new inode; never overwrite executable bytes in place.

Apple: https://developer.apple.com/documentation/security/updating-mac-software
This verifies signatures but never launches the executable or changes OS policy.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify(path):
    if sys.platform == "darwin":
        subprocess.run(
            ["/usr/bin/codesign", "--verify", "--strict", "--verbose=2", str(path)],
            check=True,
        )


def publish(source, destination):
    source = source.resolve(strict=True)
    destination = destination.absolute()
    if source == destination or destination.is_symlink():
        raise ValueError("Use a separate source and a non-symlink destination")
    destination.parent.mkdir(parents=True, exist_ok=True)
    previous_inode = destination.stat().st_ino if destination.exists() else None
    expected = digest(source)
    fd, name = tempfile.mkstemp(prefix=".dot-tui-publish-", dir=destination.parent)
    staged = Path(name)
    try:
        with os.fdopen(fd, "wb") as output, source.open("rb") as input_file:
            shutil.copyfileobj(input_file, output)
            output.flush()
            os.fsync(output.fileno())
        staged.chmod(0o755)
        if digest(staged) != expected:
            raise ValueError("Staged executable differs from source")
        verify(staged)
        # rename replaces the directory entry, preserving the old file/inode.
        # Kernel signature caches cannot become stale by an in-place truncate.
        os.replace(staged, destination)
        verify(destination)
        current_inode = destination.stat().st_ino
        if previous_inode is not None and previous_inode == current_inode:
            raise RuntimeError("Executable inode did not change")
        return {
            "source": str(source),
            "destination": str(destination),
            "sha256": digest(destination),
            "previous_inode": previous_inode,
            "current_inode": current_inode,
            "signature_verified": sys.platform == "darwin",
            "program_launched": False,
        }
    finally:
        staged.unlink(missing_ok=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    print(json.dumps(publish(args.source, args.destination), indent=2))
