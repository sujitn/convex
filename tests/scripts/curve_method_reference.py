#!/usr/bin/env python3
"""Independent Decimal(60) step-zero and integrated flat-forward controls."""
import argparse
from bisect import bisect_right
from decimal import Decimal, localcontext
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / 'tests/fixtures/library/curve-method-reference.json'
D = Decimal


def controls():
    with localcontext() as ctx:
        ctx.prec = 60
        grids = []
        for name, raw in [('positive', ['.02', '.02', '.025', '.03']),
                          ('negative', ['-.02', '-.02', '-.015', '-.01'])]:
            xs = list(map(D, ['0', '1', '2', '5']))
            ys = list(map(D, raw))
            forwards = [(xs[i+1]*ys[i+1]-xs[i]*ys[i])/(xs[i+1]-xs[i])
                        for i in range(3)]
            for method in ['PiecewiseConstant', 'FlatForward']:
                points = []
                for text in ['0', '.000001', '.5', '.999999', '1', '1.000001',
                             '1.5', '1.999999', '2', '2.000001', '3.25', '4.999999', '5']:
                    t = D(text)
                    i = min(bisect_right(xs, t)-1, 2)
                    if method == 'PiecewiseConstant':
                        z = ys[min(bisect_right(xs, t)-1, 3)]
                    else:
                        z = ys[0] if t == 0 else (xs[i]*ys[i]+forwards[i]*(t-xs[i]))/t
                    point = dict(time=text, zero=str(z), discount=str((-t*z).exp()))
                    if method == 'FlatForward':
                        point['right'] = str(forwards[i]) if t < xs[-1] else None
                        left = xs.index(t)-1 if t in xs and t > 0 else i
                        point['left'] = str(forwards[left]) if t > 0 else None
                    points.append(point)
                tails = []
                for extension in ['Flat', 'Linear', 'FlatForward']:
                    if extension == 'FlatForward' and method != 'FlatForward':
                        continue
                    for text in ['5.000001', '7', '150']:
                        t = D(text)
                        if extension == 'Flat':
                            z = ys[-1]
                        elif extension == 'Linear':
                            z = ys[-1]+(ys[-1]-ys[-2])/(xs[-1]-xs[-2])*(t-xs[-1])
                        else:
                            z = (xs[-1]*ys[-1]+forwards[-1]*(t-xs[-1]))/t
                        tails.append(dict(extension=extension,time=text,zero=str(z),
                                          discount=str((-t*z).exp())))
                def zero(t):
                    i = min(bisect_right(xs,t)-1,2)
                    return ys[min(bisect_right(xs,t)-1,3)] if method == 'PiecewiseConstant' else (xs[i]*ys[i]+forwards[i]*(t-xs[i]))/t
                a,b = D('.5'),D('3.25')
                grids.append(dict(id=f'{method}.{name}',method=method,
                                  knots=list(map(str,xs)),zeros=list(map(str,ys)),points=points,tails=tails,
                                  interval=dict(start=str(a),end=str(b),
                                                forward=str((b*zero(b)-a*zero(a))/(b-a)))))
        return dict(schema_version=1,certified=False,precision=60,
                    qualification='Independent predeclared analytic controls; no native or external engine was executed to produce expected values.',
                    absolute_rate_tolerance='1e-12',absolute_discount_tolerance='1e-12',
                    relative_discount_tolerance='1e-12',grids=grids)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--check',action='store_true')
    args = p.parse_args()
    content = (json.dumps(controls(),indent=2)+'\n').encode()
    if args.check:
        if OUTPUT.read_bytes() != content:
            raise SystemExit('Independent frozen curve-method controls differ')
    else:
        if OUTPUT.exists():
            raise SystemExit('Refusing to overwrite frozen reference')
        OUTPUT.write_bytes(content)
    print(json.dumps(dict(status='passed',grids=4,points=52,tails=30,certified=False)))


if __name__ == '__main__':
    main()
