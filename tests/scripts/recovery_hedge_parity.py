"""Fresh full CDS recovery and issuer cash/common-factor columns and actual separate-process C ABI parity."""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import subprocess


def parity(binary, library=None, output=None):
    raw = subprocess.check_output([binary, '--emit-parity']); corpus = json.loads(raw)
    folder = Path(__file__).resolve().parents[1] / 'fixtures/library'
    cases = [c for name in ['recovery-hedge-admitted.json', 'recovery-hedge-boundaries.json'] for c in json.loads((folder / name).read_bytes())['cases']]
    ids = {c['id'] for c in cases}
    original = [c for c in corpus['cases'] if c['id'] in ids]
    if corpus['certified'] is not False or len(original) != len(cases): raise ValueError('Incomplete declared full recovery and mixed-product corpus')
    for a,b in zip(cases, original):
        if any(a[k] != b[k] for k in ['id', 'request', 'expected_error']): raise ValueError('Original request/outcome changed')
    if output:
        p = Path(output)
        if p.exists(): raise ValueError('Refusing to overwrite transport evidence')
        p.write_bytes(raw)
    if library:
        native = ctypes.CDLL(str(Path(library).resolve()))
        native.convex_library.argtypes = [ctypes.c_char_p]; native.convex_library.restype = ctypes.c_void_p
        native.convex_string_free.argtypes = [ctypes.c_void_p]; native.convex_string_free.restype = None
        for c in corpus['cases']:
            p = native.convex_library(json.dumps(c['request'], separators=(',', ':')).encode())
            if not p: raise ValueError('Null actual C response: ' + c['id'])
            try: actual = json.loads(ctypes.string_at(p))
            finally: native.convex_string_free(p)
            if actual != c['expected_envelope']: raise ValueError('Actual C/native envelope differs: ' + c['id'])
    return dict(status='passed', original_requests=len(cases), complete_transport_requests=len(corpus['cases']),
                binary_sha256=hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
                c_library_sha256=hashlib.sha256(Path(library).read_bytes()).hexdigest() if library else None,
                actual_c_abi=bool(library), qualification='Fresh outputs prove transport only; independent financial controls are separately retained.', certified=False)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--binary', required=True); p.add_argument('--library'); p.add_argument('--output'); a = p.parse_args()
    print(json.dumps(parity(a.binary, a.library, a.output)))
