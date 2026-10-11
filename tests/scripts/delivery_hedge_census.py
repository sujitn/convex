"""Lossless API descriptors of the complete independently declared basket controls."""
import argparse
import hashlib
import json
from pathlib import Path
from graph_hedge_census import compact

FOLDER = Path(__file__).resolve().parents[1] / 'fixtures/library'
SOURCES = ['delivery-hedge-decimal.json', 'delivery-hedge-rebuilt.json',
           'delivery-hedge-frontier.json']


def source_cases():
    cases = [case for name in SOURCES for case in
             json.loads((FOLDER / name).read_bytes())['cases']]
    cases += json.loads((FOLDER / SOURCES[0]).read_bytes())['books'][0]['columns']
    if len(cases) != 64 or len({c['id'] for c in cases}) != 64:
        raise ValueError('Full declared basket/column scope differs')
    return cases


def spans(source, key):
    decoder = json.JSONDecoder()
    start = source.index('[', source.index('"' + key + '":'))
    values, _ = decoder.raw_decode(source, start)
    cursor = start + 1
    result = []
    for _ in values:
        while source[cursor] in ' \t\r\n,':
            cursor += 1
        _, end = decoder.raw_decode(source, cursor)
        result.append(source[cursor:end])
        cursor = end
    return result


def build():
    reads, original = {}, []
    for name in SOURCES:
        raw = (FOLDER / name).read_bytes()
        reads[name] = hashlib.sha256(raw).hexdigest()
        original += spans(raw.decode(), 'cases')
    original += spans((FOLDER / SOURCES[0]).read_text(), 'columns')
    metadata = dict(schema_version=1, certified=False, source_sha256=reads,
        qualification='Lossless descriptors of complete separately declared financial '
        'controls. Original number/string lexemes, full inputs and outcomes survive. '
        'No native output is a financial reference or certificate.')
    prefix = json.dumps(metadata)[:-1] + ',"cases":['
    parts = [compact((prefix + ','.join(original[i:i + 16]) + ']}').encode())
             for i in range(0, len(original), 16)]
    if [c for p in parts for c in json.loads(p)['cases']] != source_cases():
        raise ValueError('Original complete control bytes or ordering differ')
    return parts


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true')
    args = parser.parse_args(); parts = build()
    paths = [FOLDER / f'delivery-hedge-api-{i + 1:02d}.json' for i in range(len(parts))]
    if args.write and any(p.exists() for p in paths):
        raise ValueError('Refusing to overwrite census evidence')
    for path, data in zip(paths, parts):
        if args.write:
            path.write_bytes(data)
        elif path.read_bytes() != data:
            raise ValueError('Lossless census changed')
    print(json.dumps(dict(status='passed', cases=64, parts=len(parts),
        source_lexemes_preserved=True, census_bytes=[len(p) for p in parts],
        census_sha256=[hashlib.sha256(p).hexdigest() for p in parts], certified=False)))
