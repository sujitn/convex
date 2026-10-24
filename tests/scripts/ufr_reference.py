#!/usr/bin/env python3
"""Independent Decimal(60) exponential forward-convergence controls."""
import argparse
from bisect import bisect_left
import datetime as dt
from decimal import Decimal, localcontext
import json
from pathlib import Path

from curve_family_reference import zero as parametric_zero, forward as parametric_forward

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / 'tests/fixtures/library/ufr-reference.json'
D = Decimal


def year_fraction(start, end, basis):
    if basis != 'ActActIsda':
        return D((end-start).days)/D(360 if basis == 'Act360' else 365)
    value = D(0)
    while start < end:
        following = dt.date(start.year+1, 1, 1)
        boundary = min(following, end)
        value += D((boundary-start).days)/D((following-dt.date(start.year, 1, 1)).days)
        start = boundary
    return value


def boundary(model):
    t = D(model['llp'])
    if model['family'] == 'parametric':
        return parametric_zero(model['parameters'], t), parametric_forward(model['parameters'], t)
    xs, ys = list(map(D, model['knots'])), list(map(D, model['zeros']))
    i = bisect_left(xs, t)-1
    f = (xs[i+1]*ys[i+1]-xs[i]*ys[i])/(xs[i+1]-xs[i])
    return (xs[i]*ys[i]+f*(t-xs[i]))/t, f


def tail(t, llp, zero, forward, ufr, alpha):
    tau = t-llp
    decay = (-alpha*tau).exp()
    f = ufr+(forward-ufr)*decay
    integral = ufr*tau+(forward-ufr)*(1-decay)/alpha
    z = (zero*llp+integral)/t
    return dict(zero=str(z), forward=str(f), discount=str((-z*t).exp()))


def controls():
    with localcontext() as ctx:
        ctx.prec = 60
        models = []
        for name, zeros, ufr, alpha, llp in [
            ('positive', ['.02','.02','.025','.03'], '.042', '.1', '20'),
            ('negative', ['-.01','-.01','-.0175','-.02'], '-.005', '.3', '20'),
            ('tiny_alpha', ['.03','.03','.035','.04'], '.01', '1e-14', '20'),
            ('fast_alpha', ['.02','.02','.025','.03'], '.042', '1e6', '20'),
            ('knot_cutoff', ['.02','.02','.025','.03'], '.042', '.1', '10'),
            ('inside_cutoff', ['.02','.02','.025','.03'], '.042', '.1', '15'),
            ('flat', ['.03','.03','.03','.03'], '.03', '.1', '20'),
        ]:
            models.append(dict(id=name,family='flat_forward',knots=['0','10','20','50'],
                               zeros=zeros,llp=llp,ufr=ufr,alpha=alpha))
        models.append(dict(id='tiny_llp',family='flat_forward',knots=['0','5e-10','1e-9','1'],
                           zeros=['.02','.02','.025','.03'],llp='1e-9',ufr='.042',alpha='.1'))
        original = json.loads((ROOT/'tests/fixtures/library/curve-family-reference.json').read_bytes())
        for model in original['models']:
            models.append(dict(id=model['id'],family='parametric',
                parameters={k:v for k,v in model.items() if k!='id'},llp='20',ufr='.042',alpha='.1'))
        for model in models:
            llp = D(model['llp'])
            z, f = boundary(model)
            model.update(boundary_zero=str(z),boundary_forward=str(f),points=[])
            for t in [llp,llp+D('1e-10'),llp+D('1e-6'),llp+D(1),llp+D(10),D(60),D(150)]:
                # A JSON number is consumed as binary64; fast convergence makes
                # sub-ulp time differences material. Evaluate the exact submitted
                # binary coordinate independently, retaining the original label.
                actual_time=D.from_float(float(t))
                model['points'].append(dict(time=str(t),evaluated_time=str(actual_time),
                    **tail(actual_time,llp,z,f,D(model['ufr']),D(model['alpha']))))
        reference = dt.date(2024,2,29)
        dated = []
        for basis in ['Act365Fixed','Act360','ActActIsda']:
            dates = [reference+dt.timedelta(days=d) for d in [0,365,730,1825]]
            model = dict(id='dated.'+basis,family='flat_forward',time_basis=basis,
                         knots=[str(year_fraction(reference,x,basis)) for x in dates],
                         zeros=['.02','.02','.025','.03'],llp=str(year_fraction(reference,dates[2],basis)),
                         llp_date=str(dates[2]),dates=list(map(str,dates)),ufr='.042',alpha='.1',points=[])
            z, f = boundary(model)
            model.update(boundary_zero=str(z),boundary_forward=str(f))
            for days in [730,731,1095,3650]:
                date=reference+dt.timedelta(days=days);t=year_fraction(reference,date,basis)
                model['points'].append(dict(date=str(date),time=str(t),
                    **tail(t,D(model['llp']),z,f,D(model['ufr']),D(model['alpha']))))
            dated.append(model)
        return dict(schema_version=1,certified=False,precision=60,reference_date=str(reference),
            qualification='Independent analytic exponential forward/integral, complete flat-forward left boundaries and retained NS/Svensson equations; no native or external engine output used.',
            bounds=dict(rate_absolute='1e-12',discount_absolute='1e-12',discount_relative='1e-12'),
            models=models,dated=dated)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--check',action='store_true');args=p.parse_args()
    data=controls();raw=(json.dumps(data,indent=2)+'\n').encode()
    if args.check:
        if OUTPUT.read_bytes()!=raw:raise SystemExit('Frozen independent UFR controls differ')
    else:
        if OUTPUT.exists():raise SystemExit('Refusing to overwrite frozen financial reference')
        OUTPUT.write_bytes(raw)
    print(json.dumps(dict(status='passed',models=len(data['models']),
                         points=sum(len(x['points']) for x in data['models']+data['dated']),certified=False)))


if __name__=='__main__':main()
