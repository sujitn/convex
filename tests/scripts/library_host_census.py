#!/usr/bin/env python3
"""Retain every complete native request/envelope for actual Java host execution.

This is exact transport evidence, never a financial oracle or certificate.
Original fixtures, numerical controls and source pins are read only.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/library'))
import api


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def validate_row(original, row):
    if row['source_case'] != original:
        raise ValueError('Complete original source descriptor changed: ' + original['id'])
    if json.loads(row['request_json']) != original['request']:
        raise ValueError('Original complete native wire request changed')
    for field in ['id', 'request']:
        if row[field] != original[field]:
            raise ValueError('Complete original request/error changed: ' + original['id'])
    # The two original positive delivery book descriptors carry `expected` and
    # omit expected_error. The original native contracts resolve absence as null.
    if row['expected_error'] != original.get('expected_error'):
        raise ValueError('Original resolved error expectation changed')
    envelope = row['expected_envelope']
    success = original.get('expected_error') is None
    if envelope['ok'] != ('true' if success else 'false'):
        raise ValueError('Original complete outcome changed: ' + original['id'])
    if not success and envelope['error']['code'] != original['expected_error']:
        raise ValueError('Original typed error changed: ' + original['id'])
    if envelope['schema_version'] != 1:
        raise ValueError('Original schema version changed')


def prepare(binary, directory):
    api.check(root=ROOT)
    matrix = api.read(ROOT / api.MATRIX)
    paths = ['tests/fixtures/library/cases.json'] + matrix['supplemental_corpora']
    binary = Path(binary).resolve(strict=True)
    output = Path(directory).resolve()
    if output.exists():
        raise ValueError('Refusing to overwrite an actual host census')
    output.mkdir(parents=True)
    corpora = []
    identities = set()
    for index, name in enumerate(paths):
        original = api.read(ROOT / name)['cases']
        target = output / f'{index:03d}.jsonl.gz'
        error_path = output / f'{index:03d}.stderr'
        raw_digest = hashlib.sha256()
        raw_bytes = 0
        with error_path.open('wb') as stderr, target.open('wb') as raw_file:
            with gzip.GzipFile(filename='', mode='wb', fileobj=raw_file, mtime=0) as archive:
                with subprocess.Popen([str(binary), str(ROOT / name)], cwd=ROOT,
                                      stdout=subprocess.PIPE, stderr=stderr) as process:
                    try:
                        for case in original:
                            line = process.stdout.readline()
                            if not line or len(line) > 32 * 1024 * 1024:
                                raise ValueError('Missing or oversized full native transport row')
                            row = json.loads(line)
                            validate_row(case, row)
                            if case['id'] in identities:
                                raise ValueError('Duplicate original request identity')
                            identities.add(case['id'])
                            raw_digest.update(line)
                            raw_bytes += len(line)
                            archive.write(line)
                        if process.stdout.read(1):
                            raise ValueError('Undeclared extra native request')
                        if process.wait() != 0:
                            raise ValueError('Actual complete native consumer failed: ' + name)
                    except BaseException:
                        if process.poll() is None:
                            process.terminate()
                        process.wait()
                        raise
        corpora.append(dict(source=name, source_sha256=sha(ROOT / name),
                            file=target.name, sha256=sha(target), raw_sha256=raw_digest.hexdigest(),
                            raw_bytes=raw_bytes, cases=len(original),
                            successes=sum(c.get('expected_error') is None for c in original)))
    document = dict(schema_version=1, certified=False,
                    qualification='Every original request and full native envelope; transport only, no financial oracle or independent approval.',
                    native_binary_sha256=sha(binary), mandatory_scope_sha256=api.scope_hash(api.read(ROOT / api.INVENTORY)),
                    complete_requests=len(identities), corpora=corpora)
    (output / 'index.json').write_text(json.dumps(document, indent=2) + '\n')
    print(json.dumps(dict(status='passed', complete_requests=len(identities),
                          corpora=len(corpora), raw_bytes=sum(c['raw_bytes'] for c in corpora),
                          native_binary_sha256=document['native_binary_sha256'], certified=False)))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--output-dir', required=True)
    args = parser.parse_args()
    prepare(args.binary, args.output_dir)
