#!/usr/bin/env python3
"""Materialize the xz-packed regression fixtures.

Large JSON corpora under tests/fixtures are committed as `<name>.json.xz`.
`materialize` writes each `<name>.json` beside its archive (ignored by Git);
tests, scripts and bindings then read the plain files as before. The archive
is the source of truth: a materialized file that differs from it is rewritten.

    python3 tools/fixtures.py materialize   # before building or testing
    python3 tools/fixtures.py check         # verify archives, ignores and materialized files
    python3 tools/fixtures.py pack FILE...  # after regenerating a fixture
"""
import argparse
import json
import lzma
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'tests/fixtures'
IGNORE = FIXTURES / '.gitignore'
IGNORE_HEADER = '# Materialized by `python3 tools/fixtures.py materialize`; maintained by `pack`.\n'


def require(condition, reason):
    if not condition:
        raise SystemExit(reason)


def archived():
    return sorted(p.relative_to(ROOT).as_posix()[:-3] for p in FIXTURES.rglob('*.json.xz'))


def ignores(fixtures):
    lines = sorted(Path(p).relative_to('tests/fixtures').as_posix() for p in fixtures)
    return (IGNORE_HEADER + ''.join(f'/{line}\n' for line in lines)).encode()


def check_ignores(fixtures):
    # A Windows checkout may write the ignore file with CRLF.
    current = IGNORE.read_bytes().replace(b'\r\n', b'\n')
    require(current == ignores(fixtures), 'tests/fixtures/.gitignore is stale; run pack')


def decompress(path):
    # xz's CRC64 check rejects a corrupt archive.
    return lzma.decompress((ROOT / f'{path}.xz').read_bytes(), format=lzma.FORMAT_XZ)


def unpack(path):
    target = ROOT / path
    data = decompress(path)
    if target.is_file() and target.read_bytes() == data:
        return False
    partial = target.with_name(target.name + '.partial')
    partial.write_bytes(data)
    partial.replace(target)
    return True


def stale(path):
    target = ROOT / path
    return target.is_file() and target.read_bytes() != decompress(path)


def materialize():
    fixtures = archived()
    check_ignores(fixtures)
    # lzma releases the GIL; a few workers bound peak memory on CI runners.
    with ThreadPoolExecutor(8) as pool:
        written = sum(pool.map(unpack, fixtures))
    print(json.dumps({'status': 'materialized', 'fixtures': len(fixtures), 'written': written}))


def check():
    fixtures = archived()
    check_ignores(fixtures)
    with ThreadPoolExecutor(8) as pool:
        differ = [p for p, s in zip(fixtures, pool.map(stale, fixtures)) if s]
    require(not differ, f'materialized fixtures differ from their archives; run pack or materialize: {differ[:10]}')
    print(json.dumps({'status': 'passed', 'fixtures': len(fixtures)}))


def pack(files):
    for name in files:
        path = Path(name).resolve().relative_to(ROOT).as_posix()
        require(path.startswith('tests/fixtures/') and path.endswith('.json'), f'not a JSON fixture: {path}')
        data = (ROOT / path).read_bytes()
        (ROOT / f'{path}.xz').write_bytes(lzma.compress(data, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64,
                                                        preset=9 | lzma.PRESET_EXTREME))
    IGNORE.write_bytes(ignores(archived()))
    print(json.dumps({'status': 'packed', 'fixtures': len(files)}))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('materialize')
    commands.add_parser('check')
    commands.add_parser('pack').add_argument('files', nargs='+')
    args = parser.parse_args()
    if args.command == 'materialize':
        materialize()
    elif args.command == 'check':
        check()
    else:
        pack(args.files)


if __name__ == '__main__':
    main()
