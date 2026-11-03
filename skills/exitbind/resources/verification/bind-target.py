#!/usr/bin/env python3
# exitbind-managed-publication:v1
"""Select a Cargo target owned by one exact checkout without adopting artifacts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tomllib

MARKER = ".exitbind-cargo-checkout.json"


def distinct(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate ownership field")
        result[key] = value
    return result


def select(checkout, cache):
    if not hasattr(os, "O_NOFOLLOW"):
        raise ValueError("secure path operations unavailable")
    checkout = Path(checkout).resolve(strict=True)
    manifest = checkout / "Cargo.toml"
    if not checkout.is_dir() or manifest.is_symlink():
        raise ValueError("checkout or manifest is unsafe")
    with manifest.open("rb") as stream:
        data = tomllib.load(stream)
    if not any(isinstance(data.get(key), dict) for key in ("package", "workspace")):
        raise ValueError("checkout lacks a Cargo package or workspace")
    cache = Path(cache)
    cache_fd = os.open(cache, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        name = "cargo-" + hashlib.sha256(os.fsencode(str(checkout))).hexdigest()
        try:
            os.mkdir(name, 0o700, dir_fd=cache_fd)
        except FileExistsError:
            pass
        fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=cache_fd)
        try:
            names = os.listdir(fd)
            if not names:
                flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
                try:
                    marker_fd = os.open(MARKER, flags, 0o600, dir_fd=fd)
                except FileExistsError:
                    pass
                else:
                    with os.fdopen(marker_fd, "w", encoding="utf-8") as out:
                        json.dump({"version": 1, "checkout": str(checkout)}, out, sort_keys=True)
                        out.write("\n")
                        out.flush()
                        os.fsync(out.fileno())
            marker_fd = os.open(MARKER, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
            with os.fdopen(marker_fd, "rb") as marker:
                if not stat.S_ISREG(os.fstat(marker.fileno()).st_mode):
                    raise ValueError("ownership marker is not a regular file")
                raw = marker.read(4097)
            if len(raw) > 4096:
                raise ValueError("ownership marker is too large")
            try:
                owner = json.loads(raw, object_pairs_hook=distinct)
            except (ValueError, UnicodeError) as error:
                raise ValueError("ownership marker is malformed") from error
            if (not isinstance(owner, dict) or owner != {"version": 1, "checkout": str(checkout)}
                    or type(owner.get("version")) is not int):
                raise ValueError("target is not owned by this checkout")
        finally:
            os.close(fd)
    finally:
        os.close(cache_fd)
    return {"checkout": str(checkout), "target": str(cache.resolve() / name)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkout", required=True)
    parser.add_argument("--cache-root", required=True)
    args = parser.parse_args()
    try:
        result = select(args.checkout, args.cache_root)
    except (OSError, ValueError, tomllib.TOMLDecodeError):
        print("bind target: unsafe, unowned, or invalid checkout/target", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
