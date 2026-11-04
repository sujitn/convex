"""Fresh mixed-book scenario and attribution requests through an actual separate C library."""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import subprocess
from mixed_scenario_census import source_cases


def parity(binary, library=None, output=None):
    raw = subprocess.check_output([binary, '--emit-parity']); corpus = json.loads(raw)
    cases = source_cases()
    if corpus['certified'] is not False or len(corpus['cases']) != 48:
        raise ValueError('Incomplete full basket and hedge transport corpus')
    for a, b in zip(cases, corpus['cases']):
        if a['id'] != b['id'] or a['request'] != b['request'] or a.get('expected_error') != b['expected_error']:
            raise ValueError('Original full request/outcome changed: ' + a['id'])
    if output:
        path = Path(output)
        if path.exists(): raise ValueError('Refusing to overwrite transport evidence')
        path.write_bytes(raw)
    if library:
        native = ctypes.CDLL(str(Path(library).resolve()))
        native.convex_library.argtypes = [ctypes.c_char_p]; native.convex_library.restype = ctypes.c_void_p
        native.convex_string_free.argtypes = [ctypes.c_void_p]; native.convex_string_free.restype = None
        for case in corpus['cases']:
            pointer = native.convex_library(json.dumps(case['request'], separators=(',', ':')).encode())
            if not pointer: raise ValueError('Null C response: ' + case['id'])
            try: actual = json.loads(ctypes.string_at(pointer))
            finally: native.convex_string_free(pointer)
            if actual != case['expected_envelope']:
                raise ValueError('Actual complete C/native result differs: ' + case['id'])
    return dict(status='passed', original_requests=len(cases), complete_transport_requests=48,
        binary_sha256=hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
        c_library_sha256=hashlib.sha256(Path(library).read_bytes()).hexdigest() if library else None,
        actual_c_abi=bool(library), qualification='Native envelopes prove transport only. '
        'Financial answers are independently declared before execution.', certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--binary', required=True)
    parser.add_argument('--library'); parser.add_argument('--output'); args = parser.parse_args()
    print(json.dumps(parity(args.binary, args.library, args.output)))
