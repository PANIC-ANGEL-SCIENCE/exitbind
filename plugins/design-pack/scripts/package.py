#!/usr/bin/env python3
"""Build a deterministic, allowlisted ZIP without installing anything."""
import argparse
import hashlib
import sys
import zipfile
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_pack import ROOT, EXPECTED, check

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    output=args.output.resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the package tree')
    if output.exists():
        parser.error('output already exists; choose a new path')
    errors=check(ROOT)
    if errors:
        raise SystemExit('\n'.join(errors))
    output.parent.mkdir(parents=True,exist_ok=True)
    with zipfile.ZipFile(output,'x',compression=zipfile.ZIP_DEFLATED) as archive:
        for rel in sorted(EXPECTED):
            info=zipfile.ZipInfo('design-pack/'+rel,date_time=(2026,11,4,0,0,0))
            info.compress_type=zipfile.ZIP_DEFLATED
            info.external_attr=0o100644 << 16
            archive.writestr(info,(ROOT/rel).read_bytes())
    with zipfile.ZipFile(output) as archive:
        if archive.testzip() is not None:
            raise SystemExit('archive integrity failure')
        assert set(archive.namelist()) == {'design-pack/'+r for r in EXPECTED}
        for rel in EXPECTED:
            assert archive.read('design-pack/'+rel) == (ROOT/rel).read_bytes()
    print(f'PASS archive {output.name}: {len(EXPECTED)} files')
    print('SHA256',hashlib.sha256(output.read_bytes()).hexdigest())

if __name__ == '__main__': main()
