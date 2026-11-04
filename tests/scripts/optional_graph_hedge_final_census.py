"""Lossless bounded API census; original financial/control files stay exact."""
import argparse
import hashlib
import json
from pathlib import Path
from graph_hedge_census import compact

FOLDER = Path(__file__).resolve().parents[1] / 'fixtures/library'
SOURCES = ['optional-graph-hedge-resolved.json', 'optional-graph-hedge-direct.json', 'optional-graph-hedge-qualification.json', 'optional-graph-hedge-calibrated.json']


def build():
    cases, spans, reads = [], [], {}
    decoder = json.JSONDecoder()
    for name in SOURCES:
        raw = (FOLDER / name).read_bytes()
        reads[name] = hashlib.sha256(raw).hexdigest()
        source = raw.decode()
        start = source.index('[', source.index('"cases":'))
        original, _ = decoder.raw_decode(source, start)
        cursor = start + 1
        for case in original:
            while source[cursor] in ' \t\r\n,':
                cursor += 1
            _, end = decoder.raw_decode(source, cursor)
            if case['id'] != 'optional_graph.qualification.calibrated':
                cases.append(case)
                spans.append(source[cursor:end])
            cursor = end
    if len(cases) != 55 or len({c['id'] for c in cases}) != 55:
        raise ValueError('Complete original optional/model/book census differs')
    metadata = dict(schema_version=1, certified=False, source_sha256=reads,
        qualification='Lossless API descriptors of separately predeclared full financial '
            'controls. Case number/string lexemes, requests and expected errors are exact. '
            'No new financial answer, tolerance, model or production certificate.')
    prefix = json.dumps(metadata)[:-1] + ',"cases":['
    parts = [compact((prefix + ','.join(spans[i:i + 16]) + ']}').encode())
             for i in range(0, len(spans), 16)]
    if [c for part in parts for c in json.loads(part)['cases']] != cases:
        raise ValueError('Original full request order/values/outcomes changed')
    return parts


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    parts = build()
    paths = [FOLDER / f'optional-graph-hedge-final-census-{i + 1:02d}.json' for i in range(len(parts))]
    if args.write and any(p.exists() for p in paths):
        raise ValueError('Refusing to overwrite control/census evidence')
    for path, data in zip(paths, parts):
        if args.write:
            path.write_bytes(data)
        elif path.read_bytes() != data:
            raise ValueError('Lossless census bytes changed')
    print(json.dumps(dict(status='passed', cases=55, parts=len(parts),
        census_bytes=[len(p) for p in parts], original_json_exact=True,
        original_string_number_lexemes=True, original_input_bound=8 * 1024 * 1024,
        census_sha256=[hashlib.sha256(p).hexdigest() for p in parts], certified=False)))
