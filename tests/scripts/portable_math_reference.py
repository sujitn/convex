"""Declare independent Decimal160 controls from exact binary64 inputs.

The four-ULP kernel bound precedes candidate execution. Financial owners retain
their original bounds and checked domains. No candidate/library output is read.
"""
import argparse
from decimal import Decimal, localcontext
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'tests/fixtures/library/portable-math-reference.json'


def bits(value):
    return str(struct.unpack('>Q', struct.pack('>d', value))[0])


def generate():
    rows = []

    def add(kind, x, y=None):
        with localcontext() as ctx:
            ctx.prec = 160
            a = Decimal.from_float(float(x))
            b = Decimal.from_float(float(y)) if y is not None else None
            if kind == 'exp':
                value = a.exp()
            elif kind == 'log':
                value = a.ln()
            elif kind == 'log1p':
                # Precision must preserve tiny binary64 increments.
                ctx.prec = 520
                value = (1 + a).ln()
            elif kind == 'expm1':
                ctx.prec = 520
                value = a.exp() - 1
            elif kind == 'pow':
                value = ctx.power(a, b)
            elif kind == 'hypot':
                value = (a * a + b * b).sqrt()
            else:
                raise ValueError(kind)
            answer = float(value)
        rows.append(dict(id=f'{kind}.{len(rows)}', operation=kind,
                         x_bits=bits(float(x)), y_bits=bits(float(y)) if y is not None else None,
                         expected_bits=bits(answer), max_ulps=4))

    for x in [-746., -745., -744., -710., -709., -150., -80., -20., -4., -1.,
              -.5, -.05, -1e-12, -1e-100, -1e-300, 0., 1e-300, 1e-100,
              1e-12, .05, .5, 1., 4., 20., 80., 150., 700., 709., 710.]:
        add('exp', x)
    for x in [5e-324, 1e-320, 1e-300, 1e-100, 1e-20, 1e-12, .001, .05,
              .1, .5, .9, .9999999999999999, 1., 1.0000000000000002,
              1.01, 1.05, 1.2, 2., 10., 100., 1e20, 1e100, 1e300, 1.7976931348623157e308]:
        add('log', x)
    for x in [-.9999999999999999, -.999, -.9, -.5, -.05, -1e-10, -1e-100,
              -1e-300, 0., 1e-300, 1e-100, 1e-10, .001, .05, .5, 1., 10., 1e20, 1e300]:
        add('log1p', x)
    for x in [-746., -40., -10., -1., -.5, -.05, -1e-10, -1e-100, -1e-300,
              0., 1e-300, 1e-100, 1e-10, .001, .05, .5, 1., 10., 40., 700., 709., 710.]:
        add('expm1', x)
    for x in [1e-300, 1e-100, 1e-20, .001, .1, .5, .9, .9999999999999999,
              1., 1.0000000000000002, 1.001, 1.01, 1.05, 1.2, 2., 10., 1e20, 1e100, 1e300]:
        for y in [-150., -20., -2., -1., -.5, -1e-12, 0., 1e-12, .5, 1., 2., 20., 150.]:
            add('pow', x, y)
    for x in [-2., -1., -.5]:
        for y in [-21., -20., -1., 0., 1., 20., 21.]:
            add('pow', x, y)
    for x, y in [(0., 0.), (3., 4.), (-3., 4.), (1., 1.), (1e-300, 1e-300),
                 (1e-320, 1e-320), (1e300, 1e300), (1e308, 1e308), (1e308, 1e-308),
                 (1e-100, 1e100), (1.7976931348623157e308, 1.7976931348623157e308)]:
        add('hypot', x, y)
    return dict(schema_version=1, certified=False,
                reference='Python Decimal160 (520 for cancellation), exact input bits; no candidate input',
                bounds='Four ULP; exact classification, signed infinity/zero and nonzero sign', cases=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['write', 'check'])
    args = parser.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode()
    if args.mode == 'write':
        if TARGET.exists():
            raise ValueError('Refuse replacement of a declared reference')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Declared independent reference changed')
    print(json.dumps(dict(status='passed', primitive_controls=len(json.loads(raw)['cases']), certified=False)))


if __name__ == '__main__':
    main()
