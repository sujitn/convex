#!/usr/bin/env python3
"""Independent Decimal(60) nonnegative Hagan-West controls, without native code."""
import argparse
import datetime as dt
import json
from decimal import Decimal as D, localcontext
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT/'tests/fixtures/library/monotone-reference.json'
MODEL = 'hagan_west_2006_nonnegative_exact_axis_quadratic'


def binary(value):
    return D.from_float(float(value))


def endpoints(times, zeros):
    integrals = [t*z for t, z in zip(times, zeros)]
    means = [zeros[0]] + [(integrals[i]-integrals[i-1])/(times[i]-times[i-1])
                          for i in range(1, len(times))]
    interior = []
    for i in range(len(times)-1):
        left = times[i]-(times[i-1] if i else D(0))
        right = times[i+1]-times[i]
        interior.append((right*means[i]+left*means[i+1])/(left+right))
    raw = [D('1.5')*means[0]-interior[0]/2] + interior + [D('1.5')*means[-1]-interior[-1]/2]
    caps = [2*means[0]] + [2*min(a,b) for a,b in zip(means,means[1:])] + [2*means[-1]]
    return means, [max(D(0), min(f, c)) for f,c in zip(raw,caps)]


def shape(mean, left, right):
    g0, g1 = left-mean, right-mean
    if g0 == 0 or g1 == 0:
        # The published rational regimes are singular on these axes. This
        # explicit convention keeps the original quadratic and its primitive.
        return 'axis_quadratic', None
    if g0*g1 < 0:
        ratio = abs(g1/g0)
        if D('0.5') <= ratio <= 2:
            return 'quadratic', None
        if ratio > 2:
            return 'flat_left', (g1+2*g0)/(g1-g0)
        return 'flat_right', 3*g1/(g1-g0)
    return 'two_quadratics', g1/(g0+g1)


def local(mean, left, right, x):
    """Return forward and its exact primitive on the unit interval."""
    g0, g1 = left-mean, right-mean
    kind, eta = shape(mean, left, right)
    if kind in ['quadratic','axis_quadratic']:
        g = g0*(1-4*x+3*x*x)+g1*(-2*x+3*x*x)
        area = g0*(x-2*x*x+x*x*x)+g1*(-x*x+x*x*x)
        slope0 = -4*g0-2*g1
    elif kind == 'flat_left':
        y = max(D(0), (x-eta)/(1-eta))
        g = g0+(g1-g0)*y*y
        area = g0*x+(g1-g0)*(1-eta)*y**3/3
        slope0 = D(0)
    elif kind == 'flat_right':
        y = max(D(0), (eta-x)/eta)
        g = g1+(g0-g1)*y*y
        area = g1*x+(g0-g1)*eta*(1-y**3)/3
        slope0 = -2*(g0-g1)/eta
    else:
        shift = -g0*g1/(g0+g1)
        yl = max(D(0), (eta-x)/eta)
        yr = max(D(0), (x-eta)/(1-eta))
        g = shift+(g0-shift)*yl*yl+(g1-shift)*yr*yr
        area = shift*x+(g0-shift)*eta*(1-yl**3)/3+(g1-shift)*(1-eta)*yr**3/3
        slope0 = -2*(g0-shift)/eta
    return mean+g, mean*x+area, slope0


def evaluate(times, zeros, t):
    means, nodes = endpoints(times, zeros)
    if t == 0:
        _, _, slope = local(means[0], nodes[0], nodes[1], D(0))
        return nodes[0], nodes[0], slope/(2*times[0]), D(0)
    if t > times[-1]:
        area = times[-1]*zeros[-1]+(t-times[-1])*nodes[-1]
        zero = area/t
        return zero, nodes[-1], (nodes[-1]-zero)/t, area
    i = next(i for i, end in enumerate(times) if t <= end)
    start = times[i-1] if i else D(0)
    width = times[i]-start
    f, part, _ = local(means[i], nodes[i], nodes[i+1], (t-start)/width)
    area = (times[i-1]*zeros[i-1] if i else D(0))+width*part
    zero = area/t
    return zero, f, (f-zero)/t, area


def time(date, basis):
    start = dt.date(2024,2,29); end = dt.date.fromisoformat(date)
    if basis != 'ActActIsda':
        return D((end-start).days)/D(360 if basis == 'Act360' else 365)
    total = D(0)
    while start < end:
        boundary = min(end, dt.date(start.year+1,1,1))
        total += D((boundary-start).days)/D((dt.date(start.year+1,1,1)-dt.date(start.year,1,1)).days)
        start = boundary
    return total


def model(identity, times, zeros, dates=None, basis='Act365Fixed'):
    ts, zs = [binary(t) for t in times], [binary(z) for z in zeros]
    means, nodes = endpoints(ts,zs)
    assert all(f >= 0 for f in means)
    intervals = [dict(kind=shape(f,nodes[i],nodes[i+1])[0],
                      eta=None if shape(f,nodes[i],nodes[i+1])[1] is None else str(shape(f,nodes[i],nodes[i+1])[1]))
                 for i,f in enumerate(means)]
    coordinates = {D(0),binary(1e-10),D(60),D(150)}
    for i,end in enumerate(ts):
        start = ts[i-1] if i else D(0)
        for frac in [D(0),D('.25'),D('.5'),D('.75'),D(1)]:
            coordinates.add(binary(start+(end-start)*frac))
        eta = shape(means[i],nodes[i],nodes[i+1])[1]
        if eta is not None:
            join = start+(end-start)*eta
            for offset in [D('-1e-8'),D(0),D('1e-8')]:
                q = join+offset
                if start < q < end: coordinates.add(binary(q))
    points = []
    for t in sorted(coordinates):
        z,f,d,area = evaluate(ts,zs,t)
        assert f >= 0
        points.append(dict(time=str(t),zero=str(z),forward=str(f),derivative=str(d),integral=str(area),discount=str((-area).exp())))
    result = dict(id=identity,knots=[str(t) for t in ts],zeros=[str(z) for z in zs],
                  means=[str(f) for f in means],node_forwards=[str(f) for f in nodes],intervals=intervals,points=points,time_basis=basis)
    if dates:
        result['dates'] = dates
        result['dated_points'] = []
        for date in ['2024-03-31','2025-03-01','2026-08-15','2031-07-01']:
            t = binary(time(date,basis));z,f,d,area = evaluate(ts,zs,t)
            result['dated_points'].append(dict(date=date,time=str(t),zero=str(z),forward=str(f),derivative=str(d),integral=str(area),discount=str((-area).exp())))
    return result


def generate():
    with localcontext() as ctx:
        ctx.prec = 60
        samples = {
            'flat':[2,2,2,2], 'steep':[1,2,4,8], 'inverted':[8,4,2,1],
            'hump':[1,8,2,1], 'trough':[8,1,8,4], 'zero':[0,0,0,0],
            'zero_slice':[2,0,4,1], 'axis_up':[2,2,4,8], 'axis_down':[8,4,4,2],
            'asymmetric':[1,2,16,1], 'alternating':[16,1,16,1],
            'near_axis':[2,2.00000000001,4,8], 'tiny':[2e-6,4e-6,1e-6,2e-6],
        }
        models = []
        for identity,mean_input in samples.items():
            times = [1,2,4,8];integral = D(0); zeros = []
            for i,t in enumerate(times):
                integral += binary(mean_input[i]/128)*(D(t)-D(times[i-1] if i else 0))
                zeros.append(float(integral/D(t)))
            models.append(model(identity,times,zeros))
        models.append(model('uneven',[.125,.25,1,16],[.015625,.01953125,.03125,.04296875]))
        dates=['2024-06-30','2025-02-28','2027-02-28','2034-02-28']
        dated = [model('dated_'+basis,[time(date,basis) for date in dates],
                       [.015625,.03125,.0390625,.03515625],dates,basis)
                 for basis in ['Act365Fixed','Act360','ActActIsda']]
        return dict(schema_version=1,certified=False,model=MODEL,
            provenance='Independent Decimal(60) equations from Hagan-West 2006 (30)-(62),(81); estimates precede simultaneous positivity bounds. Exact one-zero-deviation axes use the original zero-area quadratic to preserve continuous forwards/node repricing. No amelioration. Reference does not import or execute Convex, QuantLib or ORE.',
            bounds=dict(rate_absolute='1e-12',derivative_absolute='1e-10',integral_absolute='1e-12',discount_absolute='1e-12',discount_relative='1e-12'),
            models=models,dated=dated)


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check',action='store_true');args=parser.parse_args()
    data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if args.check:
        if OUTPUT.read_bytes()!=raw:raise SystemExit('Frozen independent monotone controls differ')
    else:
        if OUTPUT.exists():raise SystemExit('Refusing to overwrite frozen controls')
        OUTPUT.write_bytes(raw)
    print(json.dumps(dict(status='passed',models=len(data['models'])+len(data['dated']),points=sum(len(m['points'])+len(m.get('dated_points',[])) for m in data['models']+data['dated']),regimes=sorted({i['kind'] for m in data['models'] for i in m['intervals']}),certified=False)))
