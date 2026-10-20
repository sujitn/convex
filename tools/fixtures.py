#!/usr/bin/env python3
"""Materialize the xz-packed regression fixtures.

Large JSON corpora under tests/fixtures are committed as `<name>.json.xz` and
listed in tests/fixtures/packed.json with the SHA-256 of their exact
decompressed bytes. `materialize` writes each `<name>.json` beside its archive
(ignored by Git) and refuses any byte that differs from the pin; tests, scripts
and bindings then read the plain files as before.

    python3 tools/fixtures.py materialize   # before building or testing
    python3 tools/fixtures.py check         # verify archives, pins and ignores
    python3 tools/fixtures.py pack FILE...  # after regenerating a fixture
"""
import argparse
import hashlib
import json
import lzma
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / 'tests/fixtures/packed.json'
IGNORE = ROOT / 'tests/fixtures/.gitignore'
IGNORE_HEADER = '# Materialized by `python3 tools/fixtures.py materialize`; maintained by `pack`.\n'


def require(condition, reason):
    if not condition:
        raise SystemExit(reason)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def load():
    data = json.loads(MANIFEST.read_bytes())
    require(data['schema_version'] == 1, 'unsupported packed fixture manifest')
    return data['fixtures']


def ignores(fixtures):
    lines = sorted(Path(p).relative_to('tests/fixtures').as_posix() for p in fixtures)
    return (IGNORE_HEADER + ''.join(f'/{line}\n' for line in lines)).encode()


def save(fixtures):
    body = {'schema_version': 1, 'fixtures': dict(sorted(fixtures.items()))}
    MANIFEST.write_bytes((json.dumps(body, indent=2) + '\n').encode())
    IGNORE.write_bytes(ignores(fixtures))


def unpack(path, pin):
    target = ROOT / path
    if target.is_file() and target.stat().st_size == pin['size'] and digest(target.read_bytes()) == pin['sha256']:
        return False
    data = lzma.decompress((ROOT / f'{path}.xz').read_bytes(), format=lzma.FORMAT_XZ)
    require(len(data) == pin['size'] and digest(data) == pin['sha256'], f'packed fixture differs from its pin: {path}')
    partial = target.with_name(target.name + '.partial')
    partial.write_bytes(data)
    partial.replace(target)
    return True


def materialize():
    fixtures = load()
    # lzma releases the GIL; a few workers bound peak memory on CI runners.
    with ThreadPoolExecutor(8) as pool:
        written = sum(pool.map(lambda item: unpack(*item), fixtures.items()))
    print(json.dumps({'status': 'materialized', 'fixtures': len(fixtures), 'written': written}))


def check():
    fixtures = load()
    archives = {p.relative_to(ROOT).as_posix()[:-3] for p in (ROOT / 'tests/fixtures').rglob('*.json.xz')}
    require(archives == set(fixtures), f'archives and pins differ: {sorted(archives ^ set(fixtures))[:10]}')
    require(IGNORE.read_bytes() == ignores(fixtures), 'tests/fixtures/.gitignore is stale; run pack')
    stale = [p for p, pin in fixtures.items() if (ROOT / p).is_file() and digest((ROOT / p).read_bytes()) != pin['sha256']]
    require(not stale, f'materialized fixtures differ from their pins: {stale[:10]}')
    print(json.dumps({'status': 'passed', 'fixtures': len(fixtures)}))


def pack(files):
    fixtures = load()
    for name in files:
        path = Path(name).resolve().relative_to(ROOT).as_posix()
        require(path.startswith('tests/fixtures/') and path.endswith('.json'), f'not a JSON fixture: {path}')
        data = (ROOT / path).read_bytes()
        (ROOT / f'{path}.xz').write_bytes(lzma.compress(data, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64,
                                                        preset=9 | lzma.PRESET_EXTREME))
        fixtures[path] = {'sha256': digest(data), 'size': len(data)}
    save(fixtures)
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
