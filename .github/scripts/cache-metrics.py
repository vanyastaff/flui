"""Report target disk costs without altering caches or build products."""

import json
import os
import shutil
from pathlib import Path


def bytes_under(root):
    total = 0
    seen = set()
    for current, _, files in os.walk(root):
        for name in files:
            metadata = (Path(current) / name).stat()
            identity = (metadata.st_dev, metadata.st_ino)
            if identity not in seen:
                seen.add(identity)
                total += metadata.st_size
    return total


if __name__ == "__main__":
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    roots = {
        "workspace_debug": target / "debug",
        "generated_consumers": target / "cli-template-check",
        "facade_consumers": target / "facade-consumer-check",
        "trybuild": target / "tests" / "trybuild",
        "ios": target / "aarch64-apple-ios",
        "cargo_registry": Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "registry",
        "cargo_git": Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "git",
    }
    print(json.dumps({
        "logical_cpus": os.cpu_count(),
        "cargo_build_jobs": os.environ.get("CARGO_BUILD_JOBS", "default"),
        "file_bytes": {key: bytes_under(path) for key, path in roots.items()},
        "disk_free_bytes": shutil.disk_usage(Path.cwd()).free,
        "note": "Unique-file logical bytes per root before rust-cache dependency cleanup, not allocated disk bytes or cache archive sizes",
    }, indent=2))
