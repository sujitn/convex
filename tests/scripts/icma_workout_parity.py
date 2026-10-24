"""Fresh native transport envelopes and actual separate-process C ABI parity."""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import subprocess

def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def parity(binary,library=None,output=None):
    raw=subprocess.check_output([binary,'--emit-parity']);corpus=json.loads(raw)
    if corpus['certified'] is not False or len(corpus['cases'])!=140:raise ValueError('Incomplete original transport corpus')
    cases=json.loads((Path(__file__).resolve().parents[1]/'fixtures/library/icma-workout-qualified.json').read_bytes())['cases']
    for expected,c in zip(cases,corpus['cases']):
        if any(expected[k]!=c[k] for k in ['id','request','expected_error']):raise ValueError('Native changed original request/outcome scope')
    if output:Path(output).write_bytes(raw)
    if library:
        native=ctypes.CDLL(str(Path(library).resolve()))
        native.convex_library.argtypes=[ctypes.c_char_p];native.convex_library.restype=ctypes.c_void_p
        native.convex_string_free.argtypes=[ctypes.c_void_p];native.convex_string_free.restype=None
        for c in corpus['cases']:
            wire=json.dumps(c['request'],separators=(',',':')).encode();p=native.convex_library(wire)
            if not p:raise ValueError('Null actual C response: '+c['id'])
            try:actual=json.loads(ctypes.string_at(p))
            finally:native.convex_string_free(p)
            if actual!=c['expected_envelope']:raise ValueError('Actual C changed native envelope: '+c['id'])
    return dict(status='passed',cases=len(cases),successes=sum(c['expected_error'] is None for c in cases),
        failures=sum(c['expected_error'] is not None for c in cases),binary_sha256=digest(binary),
        c_library_sha256=digest(library) if library else None,actual_c_abi=bool(library),
        qualification='Fresh outputs validate transport only; independent financial controls are separately declared.',certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--library');p.add_argument('--output');a=p.parse_args()
    print(json.dumps(parity(a.binary,a.library,a.output)))
