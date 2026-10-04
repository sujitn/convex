#!/usr/bin/env python3
"""Run the actual optimized host cdylib in a separate C-ABI consumer process."""
import ctypes
import json
import os
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2]
target = Path(os.environ.get('CARGO_TARGET_DIR', root / 'target')) / 'excel'
name = 'convex_ffi.dll' if sys.platform == 'win32' else 'libconvex_ffi.dylib' if sys.platform == 'darwin' else 'libconvex_ffi.so'
library = ctypes.CDLL(str(target / name))
library.convex_string_free.argtypes = [ctypes.c_void_p]
library.convex_string_free.restype = None
library.convex_release.argtypes = [ctypes.c_uint64]
library.convex_release.restype = None
library.convex_bond_from_json.argtypes = [ctypes.c_char_p]
library.convex_bond_from_json.restype = ctypes.c_uint64
library.convex_last_error.restype = ctypes.c_char_p
verbs = ['price', 'risk', 'spread', 'cashflows', 'curve_query', 'make_whole', 'yas', 'scenario', 'risk_profile', 'hedge', 'compare']

def require(condition, message):
    if not condition:
        raise RuntimeError(message)

def call(function, value):
    pointer = function(value)
    require(pointer, 'ABI returned null output')
    try:
        return json.loads(ctypes.string_at(pointer))
    finally:
        library.convex_string_free(pointer)

for verb in verbs:
    function = getattr(library, 'convex_' + verb)
    function.argtypes = [ctypes.c_char_p]
    function.restype = ctypes.c_void_p
    for value in [None, b'\xff\xfe', b'{"params":{"volatility":0.01,"volatility":0.02}}', b'{"tenor":1e300}']:
        response = call(function, value)
        require(response['ok'] == 'false' and response['error']['code'] == 'invalid_input', f'{verb}: {response}')

spec = json.dumps({'type':'fixed_rate', 'name':'CVX039-NATIVE', 'coupon_rate':0.05, 'frequency':'SemiAnnual', 'issue':'2025-01-15', 'maturity':'2035-01-15', 'day_count':'Thirty360US', 'currency':'USD', 'face_value':100}).encode()
handle = library.convex_bond_from_json(spec)
require(handle >= 100, 'valid constructor failed')
try:
    request = json.dumps({'bond':handle, 'settlement':'2025-04-15', 'mark':'99.5C'}).encode()
    response = call(library.convex_price, request)
    require(response['ok'] == 'true' and abs(response['result']['clean_price'] - 99.5) < 1e-9, 'native pricing invariant failed')
finally:
    library.convex_release(handle)
require(library.convex_bond_from_json(b' ' * (2 * 1024 * 1024) + spec) == 0, 'oversized input admitted')
require(b'limit' in library.convex_last_error(), 'missing resource rejection')
print(json.dumps({'status':'passed', 'rpc_exports_exercised':len(verbs), 'invalid_cases':len(verbs)*4, 'valid_pricing':True, 'optimized_library':str(target / name)}))
