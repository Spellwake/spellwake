"""Restore the exact operation index of a resumable Terra run from recorded source hashes."""
from contextlib import contextmanager
import hashlib
from pathlib import Path
import tarfile
import tempfile

@contextmanager
def frozen_index(harness, pins, archive_path):
    namespace = harness["indices"].__globals__
    live = namespace["ROOT"]
    if not pins:
        yield
        return
    with tempfile.TemporaryDirectory(prefix="spellwake-index-") as tmp:
        root = Path(tmp)
        with tarfile.open(archive_path) as archive:
            for name, digest in pins.items():
                path = Path(name)
                if path.is_absolute() or ".." in path.parts:
                    raise ValueError("Invalid semantic source path")
                data = (live / path).read_bytes()
                if hashlib.sha256(data).hexdigest() != digest:
                    data = archive.extractfile(name).read()
                if hashlib.sha256(data).hexdigest() != digest:
                    raise ValueError("Unavailable pinned source: " + name)
                dest = root / path
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(data)
        namespace["ROOT"] = root
        try:
            yield
        finally:
            namespace["ROOT"] = live
