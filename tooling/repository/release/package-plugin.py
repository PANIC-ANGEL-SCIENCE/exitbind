#!/usr/bin/env python3
"""Create a deterministic gzip-compressed tar archive of the plugin package."""

from __future__ import annotations

import gzip
import io
import sys
import tarfile
from pathlib import Path


def archive(package: Path, output: Path) -> None:
    root = package.name
    paths = [package, *sorted(package.rglob("*"), key=lambda path: path.relative_to(package).as_posix())]
    with output.open("wb") as destination:
        with gzip.GzipFile(
            fileobj=destination, mode="wb", filename="", mtime=0, compresslevel=9
        ) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as tar:
                for path in paths:
                    relative = path.relative_to(package).as_posix()
                    name = root if relative == "." else f"{root}/{relative}"
                    info = tarfile.TarInfo(name)
                    info.mtime = 0
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    if path.is_dir():
                        info.type = tarfile.DIRTYPE
                        info.mode = 0o755
                        tar.addfile(info)
                    elif path.is_file():
                        data = path.read_bytes()
                        info.mode = 0o644
                        info.size = len(data)
                        tar.addfile(info, io.BytesIO(data))
                    else:
                        raise ValueError(f"unsupported package entry: {path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(f"usage: {sys.argv[0]} PACKAGE OUTPUT")
    archive(Path(sys.argv[1]), Path(sys.argv[2]))
