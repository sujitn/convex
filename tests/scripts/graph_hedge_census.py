"""Two bounded whitespace-only API census parts; raw full controls stay exact."""
import argparse
import hashlib
import json
from pathlib import Path

FOLDER = Path(__file__).resolve().parents[1] / 'fixtures/library'


def compact(raw):
    out = bytearray()
    quoted = escaped = False
    for byte in raw:
        if quoted:
            out.append(byte)
            if escaped:
                escaped = False
            elif byte == 92:
                escaped = True
            elif byte == 34:
                quoted = False
        elif byte == 34:
            quoted = True
            out.append(byte)
        elif byte not in b' \t\r\n':
            out.append(byte)
    if quoted or escaped:
        raise ValueError('Incomplete source string')
    result = bytes(out) + b'\n'
    if json.loads(result) != json.loads(raw) or len(result) > 8 * 1024 * 1024:
        raise ValueError('Census JSON changed or original 8 MiB input bound exceeded')
    return result


def build():
    # Retain every original string/number lexeme; only JSON whitespace is removed.
    source = (FOLDER / 'graph-hedge-api.json').read_text()
    start = source.index('[', source.index('"cases":'))
    decoder = json.JSONDecoder()
    original, end = decoder.raw_decode(source, start)
    cursor = start + 1
    spans = []
    while len(spans) < len(original):
        while source[cursor] in ' \t\r\n,':
            cursor += 1
        _, stop = decoder.raw_decode(source, cursor)
        spans.append(source[cursor:stop])
        cursor = stop
    if len(spans) != 40:
        raise ValueError('Original full request scope differs')
    parts = [compact((source[:start] + '[' + ','.join(spans[i:i+20]) + ']' + source[end:]).encode()) for i in [0, 20]]
    joined = [case for part in parts for case in json.loads(part)['cases']]
    if joined != original:
        raise ValueError('Original case order/input/outcome changed')
    for part in parts:
        value = json.loads(part)
        value['cases'] = original
        if value != json.loads(source):
            raise ValueError('Original corpus metadata changed')
    return source.encode(), parts


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    raw, parts = build()
    paths = [FOLDER / f'graph-hedge-api-{i+1:02d}.json' for i in range(len(parts))]
    if args.write and any(p.exists() for p in paths):
        raise ValueError('Refusing to overwrite raw/census evidence')
    for path, data in zip(paths, parts):
        if args.write:
            path.write_bytes(data)
        elif path.read_bytes() != data:
            raise ValueError('Original census representation changed')
    print(json.dumps(dict(status='passed', raw_bytes=len(raw), cases=40, parts=len(parts),
        census_bytes=[len(p) for p in parts], original_json_exact=True,
        only_unquoted_whitespace_removed=True, original_input_bound=8 * 1024 * 1024,
        source_sha256=hashlib.sha256(raw).hexdigest(),
        census_sha256=[hashlib.sha256(p).hexdigest() for p in parts], certified=False)))
